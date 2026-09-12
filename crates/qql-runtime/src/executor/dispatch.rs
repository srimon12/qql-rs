use std::collections::HashMap;

use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_plan::{PlannedOperation, plan};

use crate::executor::response::{BackendResponse, ExecData};
use crate::executor::{ExecResponse, Executor, GroupedSearchResult, SearchHit};

/// Trim a grouped result set by the client-side `group_offset` (which has no
/// wire representation) exactly once.
fn trim_group_offset(groups: &mut Vec<GroupedSearchResult>, offset: Option<u64>) {
    let Some(offset) = offset else {
        return;
    };
    let offset = offset as usize;
    if offset < groups.len() {
        groups.drain(0..offset);
    } else {
        groups.clear();
    }
}

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

        // Explicit BATCH blocks run members as one forced group. The single
        // response path summarizes; per-member responses belong to the script
        // path (`execute_batch_nodes`).
        if matches!(op, PlannedOperation::Batch { .. }) {
            let mut results = Vec::new();
            self.execute_batch_op(op, true, &mut results).await?;
            let total = results.len();
            let ok_count = results.iter().filter(|r| r.ok).count();
            return Ok(ExecResponse {
                ok: ok_count == total,
                operation: "BATCH".to_string(),
                message: format!("Batch: {ok_count}/{total} operations ok"),
                data: None,
                telemetry: None,
            });
        }

        let result = self.dispatch_raw(op).await?;
        Self::normalize_planned(op, result)
    }

    /// Raw backend round-trip for one planned operation plus post-write
    /// schema-cache invalidation. The backend answers typed
    /// ([`QdrantOps::execute_planned`](crate::client::QdrantOps::execute_planned));
    /// normalization is the executor's separate, pure step.
    pub(crate) async fn dispatch_raw(
        &self,
        op: &PlannedOperation,
    ) -> Result<BackendResponse, QqlError> {
        let response = self.client.execute_planned(op).await?;
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
        Ok(response)
    }

    /// Decode + normalize a typed backend response into an `ExecResponse`.
    /// Pure (no I/O): server telemetry travels on the [`BackendResponse`],
    /// never on the error path; both `dispatch_planned` and `explain_analyze`
    /// share this single normalization.
    pub(crate) fn normalize_planned(
        op: &PlannedOperation,
        mut response: BackendResponse,
    ) -> Result<ExecResponse, QqlError> {
        let telemetry = response.telemetry.take();
        let label = op.operation_label();
        let (message, data) = match op {
            PlannedOperation::Query { .. }
            | PlannedOperation::Scroll { .. }
            | PlannedOperation::GetPoints { .. } => {
                let count = response.data.hits().map_or(0, |hits| hits.len());
                (format!("Found {count} hits"), Some(response.data))
            }
            PlannedOperation::QueryGroups { request, .. } => {
                // `group_offset` has no wire representation (it is serde-skipped
                // and absent from the gRPC proto), so the backend never applies
                // it: trim the returned groups client-side, exactly once.
                if let ExecData::Groups(groups) = &mut response.data {
                    trim_group_offset(groups, request.group_offset);
                }
                let count = response.data.groups().map_or(0, |groups| groups.len());
                (format!("Found {count} group(s)"), Some(response.data))
            }
            PlannedOperation::Count { .. } => {
                let count = response.data.count().unwrap_or(0);
                (format!("Count: {count}"), Some(ExecData::Count(count)))
            }
            PlannedOperation::Facet { .. } => {
                let count = response.data.facet().map_or(0, |hits| hits.len());
                (format!("Found {count} facet hit(s)"), Some(response.data))
            }
            PlannedOperation::ListCollections => {
                let count = response.data.collections().map_or(0, <[String]>::len);
                (format!("Found {count} collection(s)"), Some(response.data))
            }
            PlannedOperation::GetCollection { .. } => (format!("{label} ok"), Some(response.data)),
            PlannedOperation::Upsert { request, .. } => {
                let n = request.points.len();
                (
                    format!("Upserted {n} point(s)"),
                    Some(ExecData::Mutation {
                        affected: Some(n as u64),
                    }),
                )
            }
            PlannedOperation::Delete { .. }
            | PlannedOperation::UpdatePayload { .. }
            | PlannedOperation::OverwritePayload { .. }
            | PlannedOperation::ClearPayload { .. }
            | PlannedOperation::DeletePayload { .. }
            | PlannedOperation::UpdateVectors { .. }
            | PlannedOperation::DeleteVectors { .. } => (
                format!("{label} ok"),
                Some(ExecData::Mutation { affected: None }),
            ),
            PlannedOperation::ListShardKeys { .. } => {
                ("Shard keys listed".into(), Some(response.data))
            }
            PlannedOperation::GetQuotas => {
                ("Quota configuration shown".into(), Some(response.data))
            }
            PlannedOperation::SetQuotas { .. } => {
                ("Quota configuration updated".into(), Some(response.data))
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
        Ok(ExecResponse {
            ok: true,
            operation: label.into(),
            message,
            data,
            telemetry,
        })
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
            let response = self.dispatch_raw(&op).await?;
            let hits = match response.data {
                ExecData::Hits(hits) => hits,
                ExecData::Groups(_)
                | ExecData::Count(_)
                | ExecData::Facet(_)
                | ExecData::Mutation { .. }
                | ExecData::Collections(_)
                | ExecData::Collection(_)
                | ExecData::ShardKeys(_)
                | ExecData::Quotas(_) => Vec::new(),
            };
            for mut hit in hits {
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
                data: Some(ExecData::Hits(Vec::new())),
                // Client-side scoring over candidate envelopes: no single
                // server timing to attribute.
                telemetry: None,
            });
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
            // The rerank field always comes from the payload: hits no longer
            // carry a denormalized `text` mirror.
            let text = hit
                .payload
                .as_ref()
                .and_then(|p| p.get(field))
                .and_then(|v| v.as_str())
                .unwrap_or("");
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
        Ok(ExecResponse {
            ok: true,
            operation: "CROSS_RERANK".into(),
            message: format!("Found {n} hits (cross-encoder)"),
            data: Some(ExecData::Hits(out)),
            telemetry: None,
        })
    }
}
