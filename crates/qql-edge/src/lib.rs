//! # QQL Edge — zero-network vector search
//!
//! Combines [fastembed-rs] for local ONNX embedding inference with
//! [qdrant-edge] for in-process HNSW vector search. No network hops,
//! no external services, no API keys.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use qql::executor::Executor;
//! use qql_edge::{EdgeQdrant, FastEmbedder, local_executor};
//!
//! # async fn example() -> Result<(), qql_core::error::QqlError> {
//! let mut executor = local_executor("/tmp/qql-edge-data", false)?;
//!
//! // All local, all in-process
//! let resp = executor.execute(
//!     "CREATE COLLECTION docs HYBRID"
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! [fastembed-rs]: https://crates.io/crates/fastembed
//! [qdrant-edge]: https://crates.io/crates/qdrant-edge

mod backend;
mod embedder;

pub use backend::EdgeQdrant;
pub use embedder::FastEmbedder;

use qql::config::QqlConfig;
use qql::executor::Executor;
use std::sync::Arc;

/// Build a fully-local [`Executor`] backed by fastembed-rs and qdrant-edge.
///
/// - `data_dir` — persistent directory for qdrant-edge collection data.
/// - `on_disk_payload` — store payload on disk (`true`) or in memory (`false`).
///
/// The returned executor can handle QQL queries, inserts, collection
/// management, etc. — all without network access.
///
/// # Example
///
/// ```rust,no_run
/// use qql_edge::local_executor;
///
/// # async fn example() -> Result<(), qql_core::error::QqlError> {
/// let mut executor = local_executor("/tmp/qql-data", false)?;
/// let resp = executor.execute("QUERY 'hello' FROM docs LIMIT 5").await?;
/// # Ok(())
/// # }
/// ```
pub fn local_executor(
    data_dir: impl Into<std::path::PathBuf>,
    on_disk_payload: bool,
) -> Result<Executor, qql_core::error::QqlError> {
    let client = Box::new(EdgeQdrant::new(data_dir, on_disk_payload));
    let embedder = Some(Arc::new(FastEmbedder::try_default()?) as Arc<dyn qql::embedder::Embedder>);
    let config = Some(QqlConfig::default());

    Ok(Executor::with_embedder(client, config, embedder))
}
