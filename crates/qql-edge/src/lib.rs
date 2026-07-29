//! # QQL Edge — zero-network vector search
//!
//! Combines [fastembed-rs] for local ONNX embedding inference with
//! [qdrant-edge] for in-process HNSW vector search. No network hops,
//! no external services, no API keys — unless you choose an external provider.
//!
//! ## Embedder options
//!
//! | Function | Embedder | Network? |
//! |---|---|---|
//! | [`local_executor`] | fastembed (ONNX, local CPU) | ❌ none |
//! | [`local_executor_with_options`] | fastembed + model/cache selection | ❌ none |
//! | [`http_executor`] | OpenAI-compatible HTTP endpoint | ✅ provider only |
//! | [`custom_executor`] | Any `Arc<dyn Embedder>` | up to you |
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use qql_edge::local_executor;
//! use qql::executor::OnError;
//!
//! # async fn example() -> Result<(), qql_core::error::QqlError> {
//! let mut executor = local_executor("/tmp/qql-edge-data", false)?;
//! let resp = executor.execute("CREATE COLLECTION docs HYBRID", OnError::Stop).await?;
//! # Ok(())
//! # }
//! ```
//!
//! [fastembed-rs]: https://crates.io/crates/fastembed
//! [qdrant-edge]: https://crates.io/crates/qdrant-edge

mod backend;
#[cfg(feature = "fastembed-local")]
mod embedder;

pub use backend::EdgeQdrant;
#[cfg(feature = "fastembed-local")]
pub use embedder::{
    list_embedding_models, resolve_embedding_model, resolve_image_model, resolve_multi_model,
    EmbeddingModelInfo, FastEmbedder, FastEmbedderOptions,
};

use qql::config::QqlConfig;
use qql::embedder::Embedder;
use qql::executor::Executor;
use std::path::PathBuf;
use std::sync::Arc;

/// Options for [`local_executor_with_options`].
#[derive(Debug, Clone, Default)]
pub struct LocalExecutorOptions {
    /// Store payloads on disk (default: `true` when constructed via bindings;
    /// this struct defaults to `false` for Rust ergonomics matching the
    /// historical `local_executor(path, false)` tests).
    pub on_disk_payload: bool,
    /// Local ONNX dense model name. See [`resolve_embedding_model`] for accepted forms.
    /// `None` → default `BGESmallENV15` (384-d).
    #[cfg(feature = "fastembed-local")]
    pub model: Option<String>,
    /// Offline multivector model (BGE-M3 ColBERT). e.g. `"bge-m3"`.
    /// When set, `embed_multi` / multivector RERANK work with no network.
    #[cfg(feature = "fastembed-local")]
    pub multi_model: Option<String>,
    /// Offline image / CLIP vision model. e.g. `"clip-vision"` / `ClipVitB32`.
    /// Pair with dense CLIP text (`model: Some("ClipVitB32".into())`) for multimodal.
    #[cfg(feature = "fastembed-local")]
    pub image_model: Option<String>,
    /// Offline cross-encoder (`bge-reranker-base`, `BGERerankerBase`, …).
    #[cfg(feature = "fastembed-local")]
    pub reranker_model: Option<String>,
    /// Override fastembed model cache directory.
    #[cfg(feature = "fastembed-local")]
    pub cache_dir: Option<PathBuf>,
    /// Show HuggingFace download progress bars (default: `false`).
    #[cfg(feature = "fastembed-local")]
    pub show_download_progress: bool,
}

/// Build a fully-local [`Executor`] backed by fastembed-rs and qdrant-edge.
///
/// Uses the default embedding model (`BGESmallENV15`, 384-d). Prefer
/// [`local_executor_with_options`] when you need a different model or cache dir.
///
/// No network calls are made at inference time — embedding runs on-device via ONNX.
/// Models are downloaded from HuggingFace on first use and cached locally.
#[cfg(feature = "fastembed-local")]
pub fn local_executor(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
) -> Result<Executor, qql_core::error::QqlError> {
    local_executor_with_options(
        data_dir,
        LocalExecutorOptions {
            on_disk_payload,
            ..Default::default()
        },
    )
}

/// Build a fully-local [`Executor`] with explicit model / cache options.
#[cfg(feature = "fastembed-local")]
pub fn local_executor_with_options(
    data_dir: impl Into<PathBuf>,
    opts: LocalExecutorOptions,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, opts.on_disk_payload));
    let embedder = FastEmbedder::try_with_options(FastEmbedderOptions {
        model: opts.model,
        multi_model: opts.multi_model,
        image_model: opts.image_model,
        reranker_model: opts.reranker_model,
        cache_dir: opts.cache_dir,
        show_download_progress: opts.show_download_progress,
    })?;

    // Pin collection vector size to the actual model dimension. Without this,
    // CREATE COLLECTION HYBRID always falls back to the hard-coded 384 and any
    // non-default model (768 / 1024-d) silently dimension-mismatches on upsert.
    let multi_dim = embedder.multi_dimension().unwrap_or(0);
    let image_dim = embedder.image_dimension().unwrap_or(0);
    let config = QqlConfig {
        inference_mode: "local".to_string(),
        embedding_dimension: embedder.dimension(),
        embedding_model: Some(embedder.model_name().to_string()),
        multi_embedding_model: embedder.multi_model_code().map(str::to_string),
        multi_embedding_dimension: multi_dim,
        image_embedding_model: embedder.image_model_code().map(str::to_string),
        image_embedding_dimension: image_dim,
        rerank_model: embedder.reranker_model_code().map(str::to_string),
        ..Default::default()
    };

    let embedder = Some(Arc::new(embedder) as Arc<dyn Embedder>);
    Ok(Executor::with_embedder(client, Some(config), embedder))
}

/// Build an edge [`Executor`] that calls an external OpenAI-compatible embedding
/// endpoint instead of running fastembed locally.
///
/// Works with: OpenAI, Ollama (`/v1/embeddings`), Cohere, Together AI,
/// Mistral, and any other provider that follows the OpenAI embeddings spec.
///
/// - `endpoint` — full URL, e.g. `"https://api.openai.com/v1/embeddings"` or
///   `"http://localhost:11434/v1/embeddings"` for local Ollama.
/// - `api_key` — Bearer token. Pass `""` for unauthenticated local providers.
/// - `model` — model name sent in the request body, e.g. `"text-embedding-3-small"`.
/// - `dimension` — expected output dimension. Must match what the model returns.
#[cfg(feature = "http-embedding")]
pub fn http_executor(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    endpoint: impl Into<String>,
    api_key: impl Into<String>,
    model: impl Into<String>,
    dimension: usize,
) -> Result<Executor, qql_core::error::QqlError> {
    http_executor_with_options(
        data_dir,
        on_disk_payload,
        qql::embedder::HttpEmbedderOptions {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            dimension,
            ..Default::default()
        },
    )
}

/// Edge executor with OpenAI-compatible dense + optional multi/ColBERT endpoints.
#[cfg(feature = "http-embedding")]
#[allow(clippy::too_many_arguments)]
pub fn http_executor_with_multi(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    endpoint: impl Into<String>,
    api_key: impl Into<String>,
    model: impl Into<String>,
    dimension: usize,
    multi_endpoint: Option<String>,
    multi_api_key: Option<String>,
    multi_model: Option<String>,
    multi_dimension: usize,
) -> Result<Executor, qql_core::error::QqlError> {
    http_executor_with_options(
        data_dir,
        on_disk_payload,
        qql::embedder::HttpEmbedderOptions {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            dimension,
            multi_endpoint,
            multi_api_key,
            multi_model,
            multi_dimension,
            ..Default::default()
        },
    )
}

/// Edge executor from full [`HttpEmbedderOptions`] (dense + multi + image/CLIP).
#[cfg(feature = "http-embedding")]
pub fn http_executor_with_options(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    opts: qql::embedder::HttpEmbedderOptions,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, on_disk_payload));
    let config = QqlConfig {
        inference_mode: "local".to_string(),
        embedding_dimension: opts.dimension,
        embedding_model: Some(opts.model.clone()),
        embedding_endpoint: None, // edge uses the Arc embedder, not config probing
        multi_embedding_endpoint: opts.multi_endpoint.clone(),
        multi_embedding_api_key: opts.multi_api_key.clone(),
        multi_embedding_model: opts.multi_model.clone(),
        multi_embedding_dimension: opts.multi_dimension,
        image_embedding_endpoint: opts.image_endpoint.clone(),
        image_embedding_api_key: opts.image_api_key.clone(),
        image_embedding_model: opts.image_model.clone(),
        image_embedding_dimension: opts.image_dimension,
        rerank_endpoint: opts.rerank_endpoint.clone(),
        rerank_api_key: opts.rerank_api_key.clone(),
        rerank_model: opts.rerank_model.clone(),
        ..Default::default()
    };
    let embedder = qql::embedder::HttpEmbedder::try_with_options(opts)?;
    Ok(Executor::with_embedder(
        client,
        Some(config),
        Some(Arc::new(embedder) as Arc<dyn Embedder>),
    ))
}

/// Build an edge [`Executor`] with a fully custom [`Embedder`].
///
/// Use this to plug in GPU-backed embedders, caching layers, ensemble
/// embedders, or any other custom implementation.
///
/// **Callers must set `embedding_dimension` on a [`QqlConfig`] themselves** if
/// they rely on `CREATE COLLECTION HYBRID` auto-sizing — pass the config via
/// [`Executor::with_embedder`] if the defaults (384) are wrong for your model.
pub fn custom_executor(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    embedder: Arc<dyn Embedder>,
) -> Result<Executor, qql_core::error::QqlError> {
    custom_executor_with_dimension(data_dir, on_disk_payload, embedder, None)
}

/// Build an edge [`Executor`] with a custom embedder and an optional explicit
/// dense dimension. The embedder-reported dimension takes precedence.
pub fn custom_executor_with_dimension(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    embedder: Arc<dyn Embedder>,
    dimension: Option<usize>,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, on_disk_payload));
    let config = QqlConfig {
        inference_mode: "local".to_string(),
        embedding_dimension: embedder.dimension().or(dimension).unwrap_or(0),
        ..Default::default()
    };
    Ok(Executor::with_embedder(
        client,
        Some(config),
        Some(embedder),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use qql::embedder::Embedder;
    use qql::executor::OnError;
    use qql_core::error::QqlError;
    use qql_embed::sparse::SparseVector;

    struct TestEmbedder;

    #[async_trait]
    impl Embedder for TestEmbedder {
        fn dimension(&self) -> Option<usize> {
            Some(3)
        }

        async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
            Ok(vec![1.0, 0.0, 0.0])
        }

        async fn embed_sparse(&self, _text: &str, _model: &str) -> Result<SparseVector, QqlError> {
            Ok(SparseVector {
                indices: vec![1],
                values: vec![1.0],
            })
        }
    }

    #[test]
    fn custom_edge_executor_is_schema_aware_and_rejects_shards() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let data_dir =
                std::env::temp_dir().join(format!("qql-edge-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&data_dir);
            let executor =
                custom_executor(&data_dir, false, Arc::new(TestEmbedder)).expect("custom executor");

            let report = executor
                .execute("CREATE COLLECTION docs HYBRID", OnError::Stop)
                .await
                .expect("create collection");
            assert!(report.ok);
            let report = executor
                .execute(
                    "UPSERT INTO docs VALUES {id: 1, text: 'hello'}",
                    OnError::Stop,
                )
                .await
                .expect("schema-aware upsert");
            assert!(report.ok);
            let report = executor
                .execute(
                    "QUERY 'hello' FROM docs USING dense AS DENSE LIMIT 1",
                    OnError::Stop,
                )
                .await
                .expect("schema-aware query");
            assert!(report.ok);
            let report = executor
                .execute(
                    "CREATE COLLECTION dense_only (dense VECTOR(3, COSINE))",
                    OnError::Stop,
                )
                .await
                .expect("create dense-only collection");
            assert!(report.ok);
            let report = executor
                .execute(
                    "UPSERT INTO dense_only VALUES {id: 2, body: 'dense only'}",
                    OnError::Stop,
                )
                .await
                .expect("dense-only auto-embed");
            assert!(report.ok);
            let report = executor
                .execute(
                    "CREATE COLLECTION sparse_only (sparse SPARSE)",
                    OnError::Stop,
                )
                .await
                .expect("create sparse-only collection");
            assert!(report.ok);
            let report = executor
                .execute(
                    "UPSERT INTO sparse_only VALUES {id: 3, content: 'sparse only'}",
                    OnError::Stop,
                )
                .await
                .expect("sparse-only auto-embed");
            assert!(report.ok);
            let report = executor
                .execute(
                    "QUERY 'hello' FROM docs USING dense AS DENSE SHARD 'tenant-a' LIMIT 1",
                    OnError::Continue,
                )
                .await
                .expect("edge should report unsupported shard");
            assert!(!report.ok);
            assert!(report.results[0].message.contains("UNSUPPORTED-SHARD"));
            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }
}
