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
//! | [`http_executor`] | OpenAI-compatible HTTP endpoint | ✅ provider only |
//! | [`custom_executor`] | Any `Arc<dyn Embedder>` | up to you |
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use qql_edge::local_executor;
//!
//! # async fn example() -> Result<(), qql_core::error::QqlError> {
//! let mut executor = local_executor("/tmp/qql-edge-data", false)?;
//! let resp = executor.execute("CREATE COLLECTION docs HYBRID").await?;
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
pub use embedder::FastEmbedder;

use qql::config::QqlConfig;
use qql::embedder::Embedder;
use qql::executor::Executor;
use std::sync::Arc;

/// Build a fully-local [`Executor`] backed by fastembed-rs and qdrant-edge.
///
/// No network calls are made at all — embedding runs on-device via ONNX.
/// Models are downloaded from HuggingFace on first use and cached locally.
#[cfg(feature = "fastembed-local")]
pub fn local_executor(
    data_dir: impl Into<std::path::PathBuf>,
    on_disk_payload: bool,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, on_disk_payload));
    let embedder = Some(Arc::new(FastEmbedder::try_default()?) as Arc<dyn Embedder>);
    let config = Some(QqlConfig::default());
    Ok(Executor::with_embedder(client, config, embedder))
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
    data_dir: impl Into<std::path::PathBuf>,
    on_disk_payload: bool,
    endpoint: impl Into<String>,
    api_key: impl Into<String>,
    model: impl Into<String>,
    dimension: usize,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, on_disk_payload));
    let embedder = Some(Arc::new(qql::embedder::HttpEmbedder::new(
        endpoint.into(),
        api_key.into(),
        model.into(),
        dimension,
    )?) as Arc<dyn Embedder>);
    let config = Some(QqlConfig::default());
    Ok(Executor::with_embedder(client, config, embedder))
}

/// Build an edge [`Executor`] with a fully custom [`Embedder`].
///
/// Use this to plug in GPU-backed embedders, caching layers, ensemble
/// embedders, or any other custom implementation.
pub fn custom_executor(
    data_dir: impl Into<std::path::PathBuf>,
    on_disk_payload: bool,
    embedder: Arc<dyn Embedder>,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, on_disk_payload));
    let config = Some(QqlConfig::default());
    Ok(Executor::with_embedder(client, config, Some(embedder)))
}
