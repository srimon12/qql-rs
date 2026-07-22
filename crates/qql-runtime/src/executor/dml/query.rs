use serde_json;

use crate::executor::{ExecResponse, Executor, SearchHit};
use qql_core::ast;
use qql_core::error::QqlError;
use qql_plan::routing::route;

impl Executor {
    pub(crate) async fn do_query(&self, stmt: ast::QueryStmt) -> Result<ExecResponse, QqlError> {
        let is_grouped = stmt.group.is_some();
        let r = route(&ast::Stmt::Query(Box::new(stmt)));
        let result = self.client.execute_route(r).await?;
        if is_grouped
            || result
                .get("result")
                .and_then(|r| r.get("groups"))
                .is_some()
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
}

fn extract_search_hits(result: &serde_json::Value) -> Vec<SearchHit> {
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
                    p.as_object().map(|o| {
                        o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                    })
                }),
            })
            .collect(),
        None => Vec::new(),
    }
}
