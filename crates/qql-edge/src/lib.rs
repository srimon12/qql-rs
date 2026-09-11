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
mod bootstrap;
#[cfg(feature = "fastembed-local")]
mod embedder;

pub use backend::EdgeQdrant;
pub use bootstrap::{ShardSummary, inspect_shard, unpack_snapshot};
#[cfg(feature = "fastembed-local")]
pub use embedder::{
    EmbeddingModelInfo, FastEmbedder, FastEmbedderOptions, list_embedding_models,
    resolve_embedding_model, resolve_image_model, resolve_multi_model,
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
    /// Write-ahead-log segment capacity in bytes. qdrant-edge pre-allocates
    /// each WAL segment to this size (default 32 MiB), which dominates the
    /// on-disk footprint of small embedded shards.
    ///
    /// Seeds the value when a shard is created or when it exists without a
    /// persisted WAL capacity; a capacity already persisted in
    /// `edge_config.json` wins, so later opens cannot ratchet the shard's
    /// config. Hosts configure it in MiB — [`wal_segment_capacity_bytes`]
    /// performs the conversion: the CLI via `--wal-segment-mb`, Python via
    /// `wal_segment_mb`, and Node via `walSegmentMb`.
    pub wal_segment_capacity: Option<usize>,
    /// Local ONNX dense model name. See [`resolve_embedding_model`] for accepted forms.
    /// `None` → default `BGESmallENV15` (384-d).
    #[cfg(feature = "fastembed-local")]
    pub model: Option<String>,
    /// Offline sparse model (SPLADE or BGE-M3 via `SparseTextEmbedding`).
    /// e.g. `"splade"`, `"bge-m3"`. When set, sparse embedding uses real ONNX
    /// inference. `None` → local wire-compatible BM25 (Qdrant
    /// `qdrant/bm25`-identical token IDs) for sparse requests.
    #[cfg(feature = "fastembed-local")]
    pub sparse_model: Option<String>,
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
    /// Client-side BM25 `k1` for the local wire-compatible document encoder
    /// (used when no offline sparse model is configured). `None` keeps the
    /// Qdrant `qdrant/bm25` default (`1.2`). Write-path only; invalid values
    /// fail closed with `QQL-VALIDATION-CONFIG` at executor construction.
    #[cfg(feature = "fastembed-local")]
    pub bm25_k1: Option<f64>,
    /// Client-side BM25 `b` (`[0, 1]`); `None` keeps the Qdrant default
    /// (`0.75`). See [`Self::bm25_k1`].
    #[cfg(feature = "fastembed-local")]
    pub bm25_b: Option<f64>,
    /// Client-side BM25 expected average document length in tokens; `None`
    /// keeps the Qdrant default (`256`). See [`Self::bm25_k1`].
    #[cfg(feature = "fastembed-local")]
    pub bm25_avg_len: Option<f64>,
}

/// Convert a MiB WAL segment capacity into the byte count
/// [`LocalExecutorOptions::wal_segment_capacity`] expects.
///
/// Host surfaces configure the knob in whole MiB (CLI `--wal-segment-mb`,
/// Python `wal_segment_mb`, Node `walSegmentMb`). `None` keeps the engine
/// default (32 MiB); zero or a value that overflows `usize` bytes fails closed
/// with `QQL-VALIDATION-CONFIG` instead of silently producing a nonsensical
/// WAL capacity.
pub fn wal_segment_capacity_bytes(
    mb: Option<u64>,
) -> Result<Option<usize>, qql_core::error::QqlError> {
    match mb {
        None => Ok(None),
        Some(0) => Err(qql_core::error::QqlError::validation(
            "QQL-VALIDATION-CONFIG",
            "wal_segment_mb must be greater than zero; omit it for the qdrant-edge 32 MiB default",
            None,
        )),
        Some(mb) => usize::try_from(mb)
            .ok()
            .and_then(|mb| mb.checked_mul(1024 * 1024))
            .map(Some)
            .ok_or_else(|| {
                qql_core::error::QqlError::validation(
                    "QQL-VALIDATION-CONFIG",
                    "wal_segment_mb is too large for this platform",
                    None,
                )
            }),
    }
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
    let client = Box::new(
        EdgeQdrant::new(data_dir, opts.on_disk_payload)
            .with_wal_segment_capacity(opts.wal_segment_capacity),
    );
    let embedder = FastEmbedder::try_with_options(FastEmbedderOptions {
        model: opts.model,
        sparse_model: opts.sparse_model,
        multi_model: opts.multi_model,
        image_model: opts.image_model,
        reranker_model: opts.reranker_model,
        cache_dir: opts.cache_dir,
        show_download_progress: opts.show_download_progress,
        bm25_k1: opts.bm25_k1,
        bm25_b: opts.bm25_b,
        bm25_avg_len: opts.bm25_avg_len,
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
        sparse_inference_model: embedder.sparse_model_code().map(str::to_string),
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

/// Edge executor from full [`qql::embedder::HttpEmbedderOptions`] (dense + multi + image/CLIP).
#[cfg(feature = "http-embedding")]
pub fn http_executor_with_options(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    opts: qql::embedder::HttpEmbedderOptions,
) -> Result<Executor, qql_core::error::QqlError> {
    http_executor_with_options_and_wal(data_dir, on_disk_payload, None, opts)
}

/// Like [`http_executor_with_options`], with an explicit WAL segment capacity
/// (bytes) applied to every shard the executor opens or creates; `None` keeps
/// the engine default (32 MiB) for new shards and the persisted value for
/// existing ones.
#[cfg(feature = "http-embedding")]
pub fn http_executor_with_options_and_wal(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    wal_segment_capacity: Option<usize>,
    opts: qql::embedder::HttpEmbedderOptions,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(
        EdgeQdrant::new(data_dir, on_disk_payload).with_wal_segment_capacity(wal_segment_capacity),
    );
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
    custom_executor_with_storage(data_dir, on_disk_payload, None, embedder, dimension)
}

/// Like [`custom_executor_with_dimension`], with an explicit WAL segment
/// capacity (bytes) applied to every shard the executor opens or creates;
/// `None` keeps the engine default.
pub fn custom_executor_with_storage(
    data_dir: impl Into<PathBuf>,
    on_disk_payload: bool,
    wal_segment_capacity: Option<usize>,
    embedder: Arc<dyn Embedder>,
    dimension: Option<usize>,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(
        EdgeQdrant::new(data_dir, on_disk_payload).with_wal_segment_capacity(wal_segment_capacity),
    );
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

        async fn embed_sparse_query(
            &self,
            _text: &str,
            _model: &str,
        ) -> Result<SparseVector, QqlError> {
            Ok(SparseVector {
                indices: vec![1],
                values: vec![1.0],
            })
        }

        async fn embed_sparse_document(
            &self,
            _text: &str,
            _model: &str,
        ) -> Result<SparseVector, QqlError> {
            Ok(SparseVector {
                indices: vec![1],
                values: vec![1.0],
            })
        }
    }

    #[test]
    #[cfg(feature = "fastembed-local")]
    fn local_executor_options_default_has_no_sparse_model() {
        let opts = LocalExecutorOptions::default();
        assert!(opts.sparse_model.is_none());
    }

    #[test]
    #[cfg(feature = "fastembed-local")]
    fn local_executor_options_with_sparse_model() {
        let opts = LocalExecutorOptions {
            sparse_model: Some("splade".into()),
            ..Default::default()
        };
        assert_eq!(opts.sparse_model.as_deref(), Some("splade"));
    }

    #[test]
    #[cfg(feature = "fastembed-local")]
    fn local_executor_options_bm25_defaults_are_unset() {
        let opts = LocalExecutorOptions::default();
        assert_eq!(opts.bm25_k1, None);
        assert_eq!(opts.bm25_b, None);
        assert_eq!(opts.bm25_avg_len, None);
    }

    /// Executor-level end-to-end: BM25 parameters configured on the embedder
    /// must land in the sparse vector qdrant-edge actually stores for a TEXT
    /// upsert, matching `qql_embed::sparse::embed_document_with_params`.
    ///
    /// Uses the edge HTTP executor so no ONNX model is needed: dense/multi
    /// inference goes to the (unused) endpoint, while sparse document encoding
    /// is the local wire-compatible BM25 path.
    #[test]
    #[cfg(feature = "http-embedding")]
    fn configured_bm25_params_land_in_stored_sparse_vector() {
        use qql::embedder::HttpEmbedderOptions;
        use qql::executor::OnError;
        use qql_plan::{PlanVectorStruct, PlanVectorValue};

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let data_dir =
                std::env::temp_dir().join(format!("qql-edge-bm25-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&data_dir);

            let params = qql_embed::Bm25Params::new(2.0, 0.5, 4.0).expect("valid params");
            let executor = http_executor_with_options_and_wal(
                &data_dir,
                false,
                None,
                HttpEmbedderOptions {
                    endpoint: "http://127.0.0.1:9/v1/embeddings".to_string(),
                    api_key: String::new(),
                    model: "unused-dense".to_string(),
                    dimension: 3,
                    bm25_k1: Some(params.k1()),
                    bm25_b: Some(params.b()),
                    bm25_avg_len: Some(params.avg_len()),
                    ..Default::default()
                },
            )
            .expect("http edge executor");

            let report = executor
                .execute("CREATE COLLECTION bm25_docs (sparse SPARSE)", OnError::Stop)
                .await
                .expect("create collection");
            assert!(report.ok, "create failed: {report:?}");

            let text = "cat sat mat cat";
            let report = executor
                .execute(
                    &format!("UPSERT INTO bm25_docs VALUES {{id: 1, text: '{text}'}}"),
                    OnError::Stop,
                )
                .await
                .expect("upsert");
            assert!(report.ok, "upsert failed: {report:?}");

            let report = executor
                .execute(
                    "QUERY POINTS (1) FROM bm25_docs WITH VECTOR true",
                    OnError::Stop,
                )
                .await
                .expect("point lookup");
            assert!(report.ok, "lookup failed: {report:?}");
            let hits = report.results[0].hits_ref().expect("hits");
            let vector = hits[0].vector.as_ref().expect("stored vector");
            let stored = match vector {
                PlanVectorStruct::Single(v) => v,
                PlanVectorStruct::Named(map) => map.get("sparse").expect("sparse named vector"),
            };
            let (indices, values) = match stored {
                PlanVectorValue::Sparse { indices, values } => (indices, values),
                other => panic!("expected sparse vector, got {other:?}"),
            };

            let expected = qql_embed::sparse::embed_document_with_params(text, &params);
            assert_eq!(*indices, expected.indices, "stored indices must match");
            assert_eq!(values.len(), expected.values.len());
            for (got, want) in values.iter().zip(&expected.values) {
                assert!(
                    (got - want).abs() < 1e-6,
                    "stored BM25 weight {got} != configured {want}"
                );
            }

            // The configured k1/b/avg_len genuinely differ from the defaults.
            let default = qql_embed::sparse::embed_document(text);
            assert_ne!(*values, default.values);

            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn wal_segment_capacity_bytes_scales_mib() {
        assert_eq!(wal_segment_capacity_bytes(None).unwrap(), None);
        assert_eq!(wal_segment_capacity_bytes(Some(4)).unwrap(), Some(4 << 20));
    }

    #[test]
    fn wal_segment_capacity_bytes_rejects_zero_and_overflow() {
        let zero = wal_segment_capacity_bytes(Some(0)).expect_err("zero must fail closed");
        assert_eq!(zero.code, "QQL-VALIDATION-CONFIG");
        let overflow =
            wal_segment_capacity_bytes(Some(u64::MAX)).expect_err("overflow must fail closed");
        assert_eq!(overflow.code, "QQL-VALIDATION-CONFIG");
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

    #[test]
    fn edge_reads_do_not_create_missing_collections() {
        // Regression (B-2): read operations against a missing collection must
        // error with QQL-EDGE-COLLECTION-NOT-FOUND and must not materialise a
        // ghost collection on disk — matching remote 404 semantics.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let data_dir =
                std::env::temp_dir().join(format!("qql-edge-readtest-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&data_dir);
            let executor =
                custom_executor(&data_dir, false, Arc::new(TestEmbedder)).expect("custom executor");

            let cases = [
                "QUERY 'hello' FROM never_queried USING dense AS DENSE LIMIT 5",
                "SCROLL FROM never_scrolled LIMIT 5",
                "QUERY POINTS (1, 2) FROM never_points WITH PAYLOAD true",
                "COUNT FROM never_counted",
                "DELETE FROM never_deleted WHERE id = 1",
            ];
            for query in cases {
                let report = executor
                    .execute(query, OnError::Continue)
                    .await
                    .expect("read against missing collection must return a report");
                assert!(
                    !report.ok,
                    "expected failure for '{query}': {:?}",
                    report.results
                );
                assert!(
                    report.results[0]
                        .message
                        .contains("QQL-EDGE-COLLECTION-NOT-FOUND"),
                    "expected QQL-EDGE-COLLECTION-NOT-FOUND for '{query}', got {:?}",
                    report.results[0].message
                );
            }

            // No ghost collections were created on disk.
            let report = executor
                .execute("SHOW COLLECTIONS", OnError::Stop)
                .await
                .expect("list collections");
            assert!(report.ok);
            let cols = report.results[0]
                .data
                .as_ref()
                .and_then(|d| d.collections())
                .expect("collections list");
            assert!(
                cols.is_empty(),
                "read operations must not create collections: {cols:?}"
            );

            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn edge_upsert_autocreate_and_get_points_shape() {
        // Regression (B-2 preserve + B-3): an embedding UPSERT to a missing
        // collection still auto-creates it with the inferred schema, and
        // QUERY POINTS on a populated collection returns non-empty hits.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let data_dir =
                std::env::temp_dir().join(format!("qql-edge-pointstest-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&data_dir);
            let executor =
                custom_executor(&data_dir, false, Arc::new(TestEmbedder)).expect("custom executor");

            // Auto-create via embedding upsert (collection does not exist yet).
            let report = executor
                .execute(
                    "UPSERT INTO auto_docs VALUES {id: 1, text: 'hello'}, {id: 2, text: 'world'}",
                    OnError::Stop,
                )
                .await
                .expect("auto-creating upsert");
            assert!(report.ok, "auto-creating UPSERT failed: {:?}", report);

            let report = executor
                .execute("SHOW COLLECTIONS", OnError::Stop)
                .await
                .expect("list collections");
            let cols = report.results[0]
                .data
                .as_ref()
                .and_then(|d| d.collections())
                .map(<[String]>::len)
                .unwrap_or(0);
            assert_eq!(
                cols, 1,
                "UPSERT auto-create should materialise one collection"
            );

            // QUERY POINTS must surface the retrieved points (response shape
            // regression) and report the hit count.
            let report = executor
                .execute(
                    "QUERY POINTS (1, 2) FROM auto_docs WITH PAYLOAD true",
                    OnError::Stop,
                )
                .await
                .expect("points lookup");
            assert!(report.ok, "POINTS lookup failed: {:?}", report);
            assert_eq!(
                report.results[0].message, "Found 2 hits",
                "POINTS lookup should report 2 hits, got {:?}",
                report.results[0].message
            );
            let hits = report.results[0].hits_ref().expect("data");
            assert_eq!(hits.len(), 2, "POINTS lookup should return 2 hits");

            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }
}
