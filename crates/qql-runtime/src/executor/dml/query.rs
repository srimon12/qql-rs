use serde_json;

use crate::client::CollectionInfo;
use crate::executor::{ExecResponse, Executor, SearchHit};
use qql_core::ast::{self, QueryCollection, QueryExpr};
use qql_core::error::QqlError;
use qql_plan::routing::route;

impl Executor {
    /// Check that the query has a vector name when the target collection uses
    /// named vectors.  Fetches the collection schema only when `USING` is omitted
    /// — a development-time mistake.  After the first error the developer adds
    /// `USING` and the happy-path short-circuit skips every subsequent query.
    async fn ensure_vector_name(&self, collection: &str, expr: &QueryExpr) -> Result<(), QqlError> {
        // Only the five expression variants have an optional `using` field.
        let using: Option<&str> = match expr {
            QueryExpr::Nearest { using, .. }
            | QueryExpr::Recommend { using, .. }
            | QueryExpr::Context { using, .. }
            | QueryExpr::Discover { using, .. }
            | QueryExpr::RelevanceFeedback { using, .. } => using.as_deref(),
            // Variants without a `using` or with a mandatory one — nothing to check.
            _ => return Ok(()),
        };

        if using.is_some() {
            return Ok(()); // happy path — zero overhead
        }

        let info = self.client.get_collection_info(collection).await?;
        check_named_vectors(collection, &info)
    }

    pub(crate) async fn do_query(&self, stmt: ast::QueryStmt) -> Result<ExecResponse, QqlError> {
        // Validate before routing so we can give a clear error instead of
        // forwarding Qdrant's cryptic "Not existing vector name error: ".
        if let QueryCollection::Explicit(ref collection_name) = stmt.collection {
            self.ensure_vector_name(collection_name, &stmt.expression)
                .await?;
        }

        let is_grouped = stmt.group.is_some();
        let r = route(&ast::Stmt::Query(Box::new(stmt)));
        let result = self.client.execute_route(r).await?;
        if is_grouped
            || result.get("result").and_then(|r| r.get("groups")).is_some()
            || result.get("groups").is_some()
        {
            let groups_count = result
                .get("result")
                .and_then(|r| r.get("groups"))
                .or_else(|| result.get("groups"))
                .and_then(|g| g.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            return Ok(ExecResponse {
                ok: true,
                operation: "QUERY_GROUPS".to_string(),
                message: format!("Found {} group(s)", groups_count),
                data: Some(result),
            });
        }
        let hits = extract_search_hits(&result);
        Ok(ExecResponse {
            ok: true,
            operation: "QUERY".to_string(),
            message: format!("Found {} hits", hits.len()),
            data: Some(serde_json::to_value(hits).unwrap_or(serde_json::Value::Null)),
        })
    }

    pub(crate) async fn do_scroll(&self, stmt: ast::ScrollStmt) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::Scroll(Box::new(stmt)));
        let result = self.client.execute_route(r).await?;
        let hits = extract_search_hits(&result);
        Ok(ExecResponse {
            ok: true,
            operation: "SCROLL".to_string(),
            message: format!("Found {} hits", hits.len()),
            data: Some(serde_json::to_value(hits).unwrap_or(serde_json::Value::Null)),
        })
    }

    pub(crate) async fn do_delete(&self, stmt: ast::DeleteStmt) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::Delete(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "DELETE".to_string(),
            message: "Points deleted".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_clear_payload(
        &self,
        stmt: ast::ClearPayloadStmt,
    ) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::ClearPayload(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "CLEAR_PAYLOAD".to_string(),
            message: "Payload cleared".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_delete_vector(
        &self,
        stmt: ast::DeleteVectorStmt,
    ) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::DeleteVector(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "DELETE_VECTOR".to_string(),
            message: "Vector(s) deleted".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_update_vector(
        &self,
        stmt: ast::UpdateVectorStmt,
    ) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::UpdateVector(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "UPDATE_VECTOR".to_string(),
            message: "Vector updated".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_update_payload(
        &self,
        stmt: ast::UpdatePayloadStmt,
    ) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::UpdatePayload(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "UPDATE_PAYLOAD".to_string(),
            message: "Payload updated".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_upsert(&self, stmt: ast::UpsertStmt) -> Result<ExecResponse, QqlError> {
        let count = stmt.points.len();

        if let Some(ref emb) = stmt.embedding {
            let (model, is_hybrid, dense_vec, sparse_vec) = match emb {
                ast::EmbeddingSpec::Dense { model, vector } => {
                    (model.as_deref(), false, vector.as_deref(), None)
                }
                ast::EmbeddingSpec::Hybrid {
                    dense_model,
                    dense_vector,
                    sparse_vector,
                    ..
                } => (
                    dense_model.as_deref(),
                    true,
                    dense_vector.as_deref(),
                    sparse_vector.as_deref(),
                ),
            };
            self.ensure_collection_for_upsert(
                &stmt.collection,
                model,
                is_hybrid,
                dense_vec,
                sparse_vec,
            )
            .await?;
        }

        let r = route(&ast::Stmt::Upsert(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "UPSERT".to_string(),
            message: format!("Upserted {} point(s)", count),
            data: Some(serde_json::json!({"count": count})),
        })
    }

    pub(crate) async fn do_count(&self, stmt: ast::CountStmt) -> Result<ExecResponse, QqlError> {
        let r = route(&ast::Stmt::Count(Box::new(stmt)));
        let result = self.client.execute_route(r).await?;
        let point_count = result
            .get("result")
            .and_then(|r| r.get("count"))
            .and_then(|c| c.as_u64())
            .or_else(|| result.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0);
        Ok(ExecResponse {
            ok: true,
            operation: "COUNT".to_string(),
            message: format!("{} point(s)", point_count),
            data: Some(serde_json::json!({"count": point_count})),
        })
    }
}

/// If the collection has named vectors, `USING` is required and we return a
/// clear error listing the available vector names.
fn check_named_vectors(collection: &str, info: &CollectionInfo) -> Result<(), QqlError> {
    let dense: Vec<&str> = info
        .schema
        .dense_vectors
        .iter()
        .map(|s| s.as_str())
        .collect();
    let sparse: Vec<&str> = info
        .schema
        .sparse_vectors
        .iter()
        .map(|s| s.as_str())
        .collect();

    if dense.is_empty() && sparse.is_empty() {
        return Ok(()); // collection uses an unnamed default vector — no USING needed
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
        .and_then(|p| p.as_array())
        .or_else(|| result.get("points").and_then(|p| p.as_array()))
        .or_else(|| result.get("result").and_then(|r| r.as_array()));

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
                score: hit.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) as f32,
                text: hit
                    .get("payload")
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str().map(|s| s.to_string())),
                payload: hit.get("payload").and_then(|p| {
                    p.as_object()
                        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                }),
            })
            .collect(),
        None => Vec::new(),
    }
}
