use std::collections::HashMap;

use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_plan::{PlannedOperation, plan};

use crate::executor::dml::query::extract_search_hits;
use crate::executor::response::serialize_hits;
use crate::executor::telemetry::ServerTelemetry;
use crate::executor::{ExecResponse, Executor, SearchHit};

impl Executor {
    /// Execute one parsed statement under the configured timeout, returning a
    /// single response.
    pub async fn execute_node(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
        self.ensure_open()?;
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.execute_node_inner(stmt),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("operation timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.execute_node_inner(stmt).await
        }
    }

    async fn execute_node_inner(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
        let prepared = self.prepare_statement(stmt).await?;
        let planned = plan(&prepared)?;
        self.dispatch_planned(&planned).await
    }

    /// Dispatch a planned operation — gRPC goes direct, REST goes through Route.
    pub(crate) async fn dispatch_planned(
        &self,
        op: &PlannedOperation,
    ) -> Result<ExecResponse, QqlError> {
        // Client-side pair scorer: never a single Qdrant route.
        if let PlannedOperation::CrossRerank {
            collection: _,
            query,
            model,
            field,
            limit,
            offset,
            candidates,
        } = op
        {
            return self
                .execute_cross_rerank(query, model, field, *limit, *offset, candidates)
                .await;
        }

        let result = self.dispatch_raw(op).await?;
        Self::normalize_planned(op, result)
    }

    /// Raw backend round-trip for one planned operation plus post-write
    /// schema-cache invalidation. Returns the untouched backend envelope
    /// (`{result, status, time, usage?}`) so callers can time the RTT
    /// separately from decode/normalize.
    pub(crate) async fn dispatch_raw(
        &self,
        op: &PlannedOperation,
    ) -> Result<serde_json::Value, QqlError> {
        let result = self.client.execute_planned(op).await?;
        match op {
            PlannedOperation::CreateCollection { collection, .. }
            | PlannedOperation::UpdateCollection { collection, .. }
            | PlannedOperation::DropCollection { collection }
            | PlannedOperation::CreateIndex { collection, .. }
            | PlannedOperation::DropIndex { collection, .. } => {
                self.invalidate_collection_schema(collection);
            }
            _ => {}
        }
        Ok(result)
    }

    /// Decode + normalize a raw backend envelope into an `ExecResponse`.
    /// Pure (no I/O): extracts server telemetry first (lenient — absent
    /// telemetry yields `telemetry: None`, never an error), then runs the
    /// single shared normalization both `dispatch_planned` and
    /// `explain_analyze` use.
    pub(crate) fn normalize_planned(
        op: &PlannedOperation,
        mut result: serde_json::Value,
    ) -> Result<ExecResponse, QqlError> {
        let telemetry = ServerTelemetry::from_envelope_opt(&result);
        let label = op.operation_label();
        // Typed hits already in hand below (fallback arm): reused for the
        // `typed_hits` cache so `hits()` never re-parses what normalization
        // just built. The pass-through arm keeps zero-copy JSON semantics —
        // its cache fills lazily on first `hits()` instead.
        let mut typed_hits: Option<Vec<SearchHit>> = None;
        let (message, data) = match op {
            PlannedOperation::Query { .. }
            | PlannedOperation::Scroll { .. }
            | PlannedOperation::GetPoints { .. } => {
                let pts_opt = if let Some(serde_json::Value::Object(obj)) = result.get_mut("result")
                {
                    obj.remove("points")
                } else if let Some(serde_json::Value::Array(_)) = result.get("result") {
                    result.as_object_mut().and_then(|o| o.remove("result"))
                } else if let Some(obj) = result.as_object_mut() {
                    obj.remove("points")
                } else {
                    None
                };
                if let Some(pts) = pts_opt {
                    let count = pts.as_array().map(|a| a.len()).unwrap_or(0);
                    (format!("Found {count} hits"), Some(pts))
                } else {
                    let hits = extract_search_hits(&result);
                    let data = Some(serialize_hits(&hits)?);
                    typed_hits = Some(hits);
                    (
                        format!("Found {} hits", typed_hits.as_ref().map_or(0, Vec::len)),
                        data,
                    )
                }
            }
            PlannedOperation::QueryGroups { request, .. } => {
                if let Some(offset) = request.group_offset {
                    let offset = offset as usize;
                    let groups_opt = if result.get("result").is_some() {
                        result.get_mut("result").and_then(|r| r.get_mut("groups"))
                    } else {
                        result.get_mut("groups")
                    };
                    if let Some(groups) = groups_opt.and_then(|g| g.as_array_mut()) {
                        if offset < groups.len() {
                            groups.drain(0..offset);
                        } else {
                            groups.clear();
                        }
                    }
                }
                let groups_count = result
                    .get("result")
                    .and_then(|r| r.get("groups"))
                    .or_else(|| result.get("groups"))
                    .and_then(|g| g.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                (format!("Found {groups_count} group(s)"), Some(result))
            }
            PlannedOperation::Count { .. } => {
                let count = result
                    .get("result")
                    .and_then(|r| r.get("count"))
                    .and_then(|c| c.as_u64())
                    .or_else(|| result.get("count").and_then(|c| c.as_u64()))
                    .unwrap_or(0);
                (format!("Count: {count}"), Some(result))
            }
            PlannedOperation::Facet { .. } => {
                let facet_hits = result
                    .get("result")
                    .and_then(|r| r.get("hits"))
                    .cloned()
                    .or_else(|| result.get("hits").cloned())
                    .unwrap_or_else(|| serde_json::json!([]));
                let count = facet_hits.as_array().map(|a| a.len()).unwrap_or(0);
                (format!("Found {count} facet hit(s)"), Some(facet_hits))
            }
            PlannedOperation::ListCollections => {
                let count = result
                    .get("result")
                    .and_then(|value| value.get("collections"))
                    .or_else(|| result.get("collections"))
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                (format!("Found {count} collection(s)"), Some(result))
            }
            PlannedOperation::GetCollection { .. } => (format!("{label} ok"), Some(result)),
            PlannedOperation::Upsert { request, .. } => {
                let n = request.points.len();
                (
                    format!("Upserted {n} point(s)"),
                    Some(serde_json::json!({"count": n})),
                )
            }
            PlannedOperation::ListShardKeys { .. } => ("Shard keys listed".into(), Some(result)),
            PlannedOperation::GetQuotas => ("Quota configuration shown".into(), Some(result)),
            PlannedOperation::SetQuotas { .. } => {
                ("Quota configuration updated".into(), Some(result))
            }
            PlannedOperation::CrossRerank { .. } => {
                return Err(QqlError::execution(
                    "QQL-CROSS-RERANK",
                    "CROSS RERANK must be executed client-side, not via a Qdrant route",
                    None,
                ));
            }
            _ => (format!("{label} ok"), None),
        };
        let mut response = ExecResponse {
            ok: true,
            operation: label.into(),
            message,
            data,
            telemetry,
            typed_hits: std::sync::OnceLock::new(),
        };
        if let Some(hits) = typed_hits {
            response = response.with_typed_hits(hits);
        }
        Ok(response)
    }

    /// Run candidate ANN stages, score (query, doc_text) with a cross-encoder, reorder.
    async fn execute_cross_rerank(
        &self,
        query: &str,
        model: &str,
        field: &str,
        limit: u64,
        offset: u64,
        candidates: &[(String, qql_plan::QueryRequest)],
    ) -> Result<ExecResponse, QqlError> {
        let embedder = self.embedder.as_ref().ok_or_else(|| {
            QqlError::execution(
                "QQL-RERANK-CROSS",
                "CROSS RERANK requires a configured embedder with pair scoring \
                 (rerank_endpoint / edge reranker_model)",
                None,
            )
        })?;

        let mut by_key: HashMap<(String, qql_plan::PlanPointId), SearchHit> = HashMap::new();
        for (collection, request) in candidates {
            let op = PlannedOperation::Query {
                collection: collection.clone(),
                request: request.clone(),
            };
            let raw = self.client.execute_planned(&op).await?;
            for mut hit in extract_search_hits(&raw) {
                hit.collection = Some(collection.clone());
                by_key
                    .entry((collection.clone(), hit.id.clone()))
                    .or_insert(hit);
            }
        }

        if by_key.is_empty() {
            return Ok(ExecResponse {
                ok: true,
                operation: "CROSS_RERANK".into(),
                message: "Found 0 hits".into(),
                data: Some(serde_json::json!([])),
                // Client-side scoring over candidate envelopes: no single
                // server timing to attribute.
                telemetry: None,
                typed_hits: std::sync::OnceLock::new(),
            }
            .with_typed_hits(Vec::new()));
        }

        let mut hits: Vec<SearchHit> = by_key.into_values().collect();
        hits.sort_by(|a, b| {
            a.collection
                .as_deref()
                .unwrap_or("")
                .cmp(b.collection.as_deref().unwrap_or(""))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut docs = Vec::with_capacity(hits.len());
        let mut keep_idx = Vec::with_capacity(hits.len());
        for (i, hit) in hits.iter().enumerate() {
            let from_payload = hit
                .payload
                .as_ref()
                .and_then(|p| p.get(field))
                .and_then(|v| v.as_str());
            let text = match from_payload {
                Some(s) if !s.is_empty() => s,
                _ if field.eq_ignore_ascii_case("text") => hit.text.as_deref().unwrap_or(""),
                _ => "",
            };
            if text.is_empty() {
                continue;
            }
            docs.push(text.to_string());
            keep_idx.push(i);
        }
        if docs.is_empty() {
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS-FIELD",
                format!(
                    "CROSS RERANK found candidates but none had non-empty payload field '{field}'. \
                     Ensure UPSERT stores text on that field and PREFETCH returns WITH PAYLOAD."
                ),
                None,
            ));
        }

        let scores = embedder.rerank_pairs(query, &docs, model).await?;
        if scores.len() != docs.len() {
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS",
                format!(
                    "rerank_pairs returned {} scores for {} documents",
                    scores.len(),
                    docs.len()
                ),
                None,
            ));
        }

        let mut ranked: Vec<(f32, SearchHit)> = keep_idx
            .into_iter()
            .zip(scores)
            .map(|(i, score)| {
                let mut h = hits[i].clone();
                h.score = score;
                (score, h)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let skip = offset as usize;
        let take = limit as usize;
        let out: Vec<SearchHit> = ranked
            .into_iter()
            .skip(skip)
            .take(take)
            .map(|(_, h)| h)
            .collect();
        let n = out.len();
        let data = Some(serialize_hits(&out)?);
        Ok(ExecResponse {
            ok: true,
            operation: "CROSS_RERANK".into(),
            message: format!("Found {n} hits (cross-encoder)"),
            data,
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        }
        .with_typed_hits(out))
    }
}
