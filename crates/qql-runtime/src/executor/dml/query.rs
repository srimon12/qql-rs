use crate::client::CollectionInfo;
use crate::executor::Executor;
use qql_core::ast::QueryStmt;
use qql_core::error::QqlError;
use qql_embed::{TopologyNames, query_needs_kind_resolution, resolve_query_vector_kinds};

impl Executor {
    /// Resolve omitted vector names from the collection schema and validate
    /// explicit names before text is embedded.
    pub(crate) async fn configure_query_vectors(
        &self,
        collection: &str,
        query: &mut QueryStmt,
    ) -> Result<(), QqlError> {
        if !query_needs_kind_resolution(query) {
            return Ok(());
        }
        let topology = self.get_cached_topology(collection).await?;
        resolve_query_vector_kinds(collection, query, &topology)
    }
}

pub(crate) fn topology_names_from_info(info: &CollectionInfo) -> TopologyNames {
    let dense = if info.schema.vectors.is_empty() {
        info.schema.dense_vectors.clone()
    } else {
        info.schema
            .vectors
            .iter()
            .map(|vector| vector.name.clone().unwrap_or_default())
            .collect()
    };
    let sparse = info
        .schema
        .sparse_vectors
        .iter()
        .map(|vector| vector.name.clone())
        .collect();
    let multivector = info
        .schema
        .vectors
        .iter()
        .filter(|v| v.multivector.is_some())
        .filter_map(|v| v.name.clone())
        .collect();
    TopologyNames {
        dense,
        sparse,
        multivector,
    }
}
