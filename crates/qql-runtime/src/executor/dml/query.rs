use crate::client::CollectionInfo;
use crate::executor::{Executor, SearchHit};
use qql_core::ast::QueryStmt;
use qql_core::error::QqlError;
use qql_embed::{query_needs_kind_resolution, resolve_query_vector_kinds, TopologyNames};

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
        let info = self.client.get_collection_info(collection).await?;
        let topology = topology_names_from_info(&info);
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

pub(crate) fn extract_search_hits(result: &serde_json::Value) -> Vec<SearchHit> {
    let points = result
        .get("result")
        .and_then(|r| r.get("points"))
        .and_then(serde_json::Value::as_array);

    match points {
        Some(pts) => pts
            .iter()
            .map(|hit| SearchHit {
                id: hit
                    .get("id")
                    .map(|id| match id {
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => id.to_string(),
                    })
                    .unwrap_or_default(),
                score: hit
                    .get("score")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0) as f32,
                text: hit
                    .get("payload")
                    .and_then(|p| p.get("text"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                payload: hit.get("payload").and_then(|p| {
                    p.as_object()
                        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                }),
            })
            .collect(),
        None => Vec::new(),
    }
}
