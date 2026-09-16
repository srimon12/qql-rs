use std::collections::HashMap;

use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_plan::{PlannedOperation, plan};

use crate::executor::response::{BackendResponse, ExecData, score_f64};
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
                self.invalidate_collection_schema(collection).await;
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

        // Deduplicate candidates by (collection, id) without cloning
        // collection names per hit: hits live in one Vec, `seen` maps to
        // indices, and names resolve once at materialization.
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut hit_coll: Vec<usize> = Vec::new();
        let mut seen: HashMap<(usize, qql_plan::PlanPointId), usize> = HashMap::new();
        for (ci, (collection, request)) in candidates.iter().enumerate() {
            let op = PlannedOperation::Query {
                collection: collection.clone(),
                request: request.clone(),
            };
            let response = self.dispatch_raw(&op).await?;
            let batch = match response.data {
                ExecData::Hits(hits) => hits,
                // Candidates are always planned queries, so any other shape
                // is a backend contract break — fail closed instead of
                // silently scoring zero documents.
                _ => {
                    return Err(QqlError::backend(
                        "QQL-BACKEND-ENVELOPE",
                        format!(
                            "CROSS RERANK candidate query on '{collection}' returned a non-hits response"
                        ),
                        None,
                    ));
                }
            };
            for hit in batch {
                // `insert` needs an owned key: one id clone per hit (the
                // per-hit collection clone is gone).
                if seen.insert((ci, hit.id.clone()), hits.len()).is_none() {
                    hit_coll.push(ci);
                    hits.push(hit);
                }
            }
        }

        if hits.is_empty() {
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

        // Deterministic pre-order (collection, id) over indices: ties in
        // rerank scores keep this order through the stable index sort below.
        let mut order: Vec<usize> = (0..hits.len()).collect();
        order.sort_by(|&a, &b| {
            candidates[hit_coll[a]]
                .0
                .cmp(&candidates[hit_coll[b]].0)
                .then_with(|| hits[a].id.cmp(&hits[b].id))
        });

        // Borrow rerank texts out of the hits; only surviving documents pay
        // for owned Strings at the `&[String]` trait boundary below.
        let mut doc_idx: Vec<usize> = Vec::with_capacity(order.len());
        let mut doc_refs: Vec<&str> = Vec::with_capacity(order.len());
        for &i in &order {
            // The rerank field always comes from the payload: hits no longer
            // carry a denormalized `text` mirror.
            let text = hits[i]
                .payload
                .as_ref()
                .and_then(|p| p.get(field))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if text.is_empty() {
                continue;
            }
            doc_idx.push(i);
            doc_refs.push(text);
        }
        if doc_refs.is_empty() {
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS-FIELD",
                format!(
                    "CROSS RERANK found candidates but none had non-empty payload field '{field}'. \
                     Ensure UPSERT stores text on that field and PREFETCH returns WITH PAYLOAD."
                ),
                None,
            ));
        }

        let docs: Vec<String> = doc_refs.iter().map(|s| s.to_string()).collect();
        let scores = embedder.rerank_pairs(query, &docs, model).await?;
        if scores.len() != doc_refs.len() {
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS",
                format!(
                    "rerank_pairs returned {} scores for {} documents",
                    scores.len(),
                    doc_refs.len()
                ),
                None,
            ));
        }

        // Rank on the stored rounded value: `ranked` holds the f64 that lands
        // on the hit, so output order always matches the visible scores
        // (sorting the raw f32 pairs could disagree with the rounded values
        // on near-ties). Losers are never cloned — paginate the index run
        // and materialize only the output slice below.
        let mut ranked: Vec<(f64, usize)> = scores
            .into_iter()
            .enumerate()
            .map(|(k, s)| (score_f64(s), k))
            .collect();
        // NaN sorts last, by explicit choice: cross-encoder scores should
        // never be NaN, but `partial_cmp` leaves NaN unordered against
        // everything, so the old `unwrap_or(Equal)` pinned each NaN at its
        // arrival slot and could surface it mid-ranking. Sinking NaNs keeps
        // the ranked prefix meaningful; the stable sort preserves arrival
        // order among NaNs (and among ties).
        ranked.sort_by(|a, b| match (a.0.is_nan(), b.0.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal),
        });

        // Pagination applies after empty-field filtering: candidates lacking
        // the rerank field are dropped before OFFSET/LIMIT, so page boundaries
        // shift with payload quality (documented contract, not wire parity).
        let skip = usize::try_from(offset).map_err(|_| {
            QqlError::validation(
                "QQL-VALIDATION-LIMIT-OVERFLOW",
                format!("CROSS RERANK OFFSET {offset} overflows pointer width; reduce OFFSET"),
                None,
            )
        })?;
        let take = usize::try_from(limit).map_err(|_| {
            QqlError::validation(
                "QQL-VALIDATION-LIMIT-OVERFLOW",
                format!("CROSS RERANK LIMIT {limit} overflows pointer width; reduce LIMIT"),
                None,
            )
        })?;
        let out: Vec<SearchHit> = ranked
            .into_iter()
            .skip(skip)
            .take(take)
            .map(|(score, k)| {
                let i = doc_idx[k];
                let mut h = hits[i].clone();
                // Already rounded at rank time: the sort key and the stored
                // hit are the same value.
                h.score = score;
                h.collection = Some(candidates[hit_coll[i]].0.clone());
                h
            })
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
