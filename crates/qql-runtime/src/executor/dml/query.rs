use crate::client::CollectionInfo;
use crate::executor::{Executor, SearchHit};
use qql_core::ast::QueryExpr;
use qql_core::error::QqlError;

impl Executor {
    /// Check that the query has a vector name when the target collection uses
    /// named vectors. Fetches the collection schema only when `USING` is
    /// omitted.
    pub(crate) async fn ensure_vector_name(
        &self,
        collection: &str,
        expr: &QueryExpr,
    ) -> Result<(), QqlError> {
        let using: Option<&str> = match expr {
            QueryExpr::Nearest { using, .. }
            | QueryExpr::Recommend { using, .. }
            | QueryExpr::Context { using, .. }
            | QueryExpr::Discover { using, .. }
            | QueryExpr::RelevanceFeedback { using, .. } => using.as_deref(),
            _ => return Ok(()),
        };

        if using.is_some() {
            return Ok(());
        }

        let info = self.client.get_collection_info(collection).await?;
        check_named_vectors(collection, &info)
    }
}

/// If the collection has named vectors, `USING` is required and the error
/// lists the available vector names.
fn check_named_vectors(collection: &str, info: &CollectionInfo) -> Result<(), QqlError> {
    let dense: Vec<&str> = info
        .schema
        .dense_vectors
        .iter()
        .map(String::as_str)
        .collect();
    let sparse: Vec<&str> = info
        .schema
        .sparse_vectors
        .iter()
        .map(String::as_str)
        .collect();

    if dense.is_empty() && sparse.is_empty() {
        return Ok(());
    }

    let mut names = dense;
    names.extend(sparse);
    Err(QqlError::execution(
        "QQL-MISSING-USING",
        format!(
            "Collection '{}' has named vectors but no USING clause was specified. \
             Add USING <vector_name> to your query. Available vectors: {}",
            collection,
            names.join(", "),
        ),
        None,
    ))
}

pub(crate) fn extract_search_hits(result: &serde_json::Value) -> Vec<SearchHit> {
    let points = result
        .get("result")
        .and_then(|r| r.get("points"))
        .and_then(serde_json::Value::as_array)
        .or_else(|| result.get("points").and_then(serde_json::Value::as_array))
        .or_else(|| result.get("result").and_then(serde_json::Value::as_array));

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
