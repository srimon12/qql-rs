use crate::client::CollectionInfo;
use crate::executor::{Executor, SearchHit};
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
        .and_then(serde_json::Value::as_array)
        // `/points/query/batch` answers one QueryResponse per search, and per
        // the OpenAPI `QueryResponse` schema each item carries the points at
        // its TOP LEVEL: `{"points": [...]}` — no `result` wrapper. Without
        // this branch every same-collection QUERY batch silently reports
        // 0 hits.
        .or_else(|| result.get("points").and_then(serde_json::Value::as_array))
        // `POST /collections/{c}/points` (get points by ID) returns `result`
        // as a bare array of point records.
        .or_else(|| result.get("result").and_then(serde_json::Value::as_array));

    match points {
        Some(pts) => pts
            .iter()
            .map(|hit| SearchHit {
                id: hit
                    .get("id")
                    .and_then(|id| match id {
                        serde_json::Value::Number(n) => {
                            if let Some(u) = n.as_u64() {
                                Some(qql_plan::PlanPointId::Number(u))
                            } else {
                                Some(qql_plan::PlanPointId::String(n.to_string()))
                            }
                        }
                        serde_json::Value::String(s) => {
                            Some(qql_plan::PlanPointId::String(s.clone()))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| qql_plan::PlanPointId::String(String::new())),
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
                collection: None,
            })
            .collect(),
        None => Vec::new(),
    }
}
