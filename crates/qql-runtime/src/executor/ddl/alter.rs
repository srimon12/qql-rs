//! `ALTER COLLECTION` preparation: fail-fast vector-name validation.

use crate::executor::Executor;
use qql_core::ast::AlterCollectionStmt;
use qql_core::error::QqlError;

impl Executor {
    /// Validate `ALTER COLLECTION` per-vector diff names against the cached
    /// collection schema before the PATCH leaves the process.
    ///
    /// The schema is fetched through the same cache query vector resolution
    /// uses, so a repeated ALTER costs no extra `GET /collections/{name}`.
    /// Unknown names fail with `QQL-UNKNOWN-VECTOR` and the available list;
    /// the backend remains the final authority for any name the schema does
    /// not expose (e.g. an empty mock schema).
    pub(crate) async fn configure_alter_collection(
        &self,
        alter: &AlterCollectionStmt,
    ) -> Result<(), QqlError> {
        let Some(config) = alter.config.as_ref() else {
            return Ok(());
        };
        // The collection-scoped `WITH VECTOR (…)` form addresses the default
        // unnamed vector (`""` on the named PATCH map).
        let checks_default = config.vectors.is_some();
        if !checks_default
            && config.vector_diffs.is_empty()
            && config.sparse_vector_diffs.is_empty()
        {
            return Ok(());
        }
        let topology = self.get_cached_topology(&alter.collection).await?;
        // An empty topology means the schema did not expose any vector (offline
        // / empty mock schemas): leave the final say to the backend instead of
        // rejecting every name.
        if topology.dense.is_empty() && topology.sparse.is_empty() {
            return Ok(());
        }
        if checks_default && !topology.dense.iter().any(String::is_empty) {
            return Err(unknown_alter_vector(&alter.collection, "", &topology.dense));
        }
        for diff in &config.vector_diffs {
            if !topology.dense.contains(&diff.name) {
                return Err(unknown_alter_vector(
                    &alter.collection,
                    &diff.name,
                    &topology.dense,
                ));
            }
        }
        for diff in &config.sparse_vector_diffs {
            if !topology.sparse.contains(&diff.name) {
                return Err(unknown_alter_vector(
                    &alter.collection,
                    &diff.name,
                    &topology.sparse,
                ));
            }
        }
        Ok(())
    }
}

/// `QQL-UNKNOWN-VECTOR` for an `ALTER COLLECTION` diff name.
fn unknown_alter_vector(collection: &str, name: &str, available: &[String]) -> QqlError {
    let names = available
        .iter()
        .map(|candidate| {
            if candidate.is_empty() {
                "<default>".to_string()
            } else {
                candidate.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    QqlError::execution(
        "QQL-UNKNOWN-VECTOR",
        format!(
            "Collection '{collection}' has no vector named '{}'. Available vectors: {}",
            if name.is_empty() { "<default>" } else { name },
            names
        ),
        None,
    )
}
