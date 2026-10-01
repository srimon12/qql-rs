//! Batch grouping, extraction, and validation for multi-statement execution.

use crate::plan::PlannedOperation;
use crate::types::{QueryBatchRequest, ReadConsistencyParam, UpdateBatchRequest};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use qql_core::ast::{QueryCollection, QueryExpr, Stmt};
use qql_core::error::QqlError;

/// Batch grouping key identifying contiguous operations that can be combined
/// into a single Qdrant batch RPC.
///
/// Batch RPCs carry **one** header opts pair (`?timeout=`/`?consistency=` for
/// queries, `?wait=` for mutations), so per-statement execution opts are part
/// of the key: statements with differing opts never co-group and each group
/// executes with its own registered opts. Without this, ambient batching would
/// silently change a statement's semantics (differing members would be forced
/// onto one header value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchKey {
    /// Query batch for the named collection with its shared read opts.
    Query {
        /// Target collection.
        collection: String,
        /// Shared read timeout in seconds (`?timeout=`).
        timeout: Option<u64>,
        /// Shared read consistency (`?consistency=`).
        consistency: Option<ReadConsistencyParam>,
    },
    /// Mutation batch for the named collection with its shared effective wait.
    Mutation {
        /// Target collection.
        collection: String,
        /// Effective `?wait=` (explicit `WAIT` or the per-kind default).
        wait: bool,
    },
}

impl BatchKey {
    /// Target collection of this batch.
    pub fn collection(&self) -> &str {
        match self {
            BatchKey::Query { collection, .. } | BatchKey::Mutation { collection, .. } => {
                collection
            }
        }
    }

    /// Family + collection identity, ignoring per-statement execution opts.
    ///
    /// Explicit `BATCH { … }` blocks compare members with this projection:
    /// member `WAIT` / read opts are rejected or owned by the header, so the
    /// per-kind defaults must never split a declared batch.
    pub fn same_family(&self, other: &BatchKey) -> bool {
        match (self, other) {
            (BatchKey::Query { collection: a, .. }, BatchKey::Query { collection: b, .. })
            | (
                BatchKey::Mutation { collection: a, .. },
                BatchKey::Mutation { collection: b, .. },
            ) => a == b,
            _ => false,
        }
    }
}

/// Effective `?wait=` a mutation statement plans with: the explicit `WAIT`
/// when present, else the per-kind planner default — `UPSERT` waits when it
/// carries embedding work (embedding resolution must complete before the
/// response), every other mutation waits. `None` for non-mutation statements.
///
/// Single source of truth shared by the planner (`PlannedOperation.wait`)
/// and the raw-AST batch key, so both grouping levels agree before planning.
pub fn statement_wait(stmt: &Stmt) -> Option<bool> {
    match stmt {
        Stmt::Upsert(stmt) => Some(
            stmt.wait
                .unwrap_or(stmt.embedding.is_some() || !stmt.embed.is_empty()),
        ),
        Stmt::Delete(stmt) => Some(stmt.wait.unwrap_or(true)),
        Stmt::UpdatePayload(stmt) => Some(stmt.wait.unwrap_or(true)),
        Stmt::ClearPayload(stmt) => Some(stmt.wait.unwrap_or(true)),
        Stmt::DeletePayload(stmt) => Some(stmt.wait.unwrap_or(true)),
        Stmt::UpdateVector(stmt) => Some(stmt.wait.unwrap_or(true)),
        Stmt::DeleteVector(stmt) => Some(stmt.wait.unwrap_or(true)),
        _ => None,
    }
}

/// Batch grouping key for a raw AST statement (before preparation/planning).
///
/// Returns `None` for statements that are never batchable (DDL, SHOW, group
/// queries, point-ID lookups, etc.).
pub fn statement_batch_key(stmt: &Stmt) -> Option<BatchKey> {
    let wait = statement_wait(stmt).unwrap_or(true);
    match stmt {
        Stmt::Query(query)
            if query.group.is_none()
                && !matches!(query.expression, QueryExpr::Points { .. })
                && !matches!(query.expression, QueryExpr::CrossRerank { .. }) =>
        {
            match &query.collection {
                QueryCollection::Explicit(collection) => {
                    let (timeout, consistency) = match &query.params {
                        Some(params) => (
                            params.timeout,
                            params.consistency.as_ref().map(ReadConsistencyParam::from),
                        ),
                        None => (None, None),
                    };
                    Some(BatchKey::Query {
                        collection: collection.clone(),
                        timeout,
                        consistency,
                    })
                }
                QueryCollection::Inherited => None,
            }
        }
        Stmt::Upsert(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        Stmt::Delete(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        Stmt::UpdatePayload(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        Stmt::ClearPayload(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        Stmt::DeletePayload(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        Stmt::UpdateVector(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        Stmt::DeleteVector(stmt) => Some(BatchKey::Mutation {
            collection: stmt.collection.clone(),
            wait,
        }),
        // Explicit BATCH blocks are already one unit: they never merge into
        // ambient groups (the executors intercept them before the grouper).
        Stmt::Batch(_) => None,
        _ => None,
    }
}

/// Typed view of one Qdrant batch response item.
///
/// Batch envelopes stay JSON at the REST boundary (per-item results have no
/// OpenAPI struct in this transport-neutral crate), so this borrows the
/// `status` / `error` strings instead of taking ownership.
#[derive(Debug, Clone, Copy)]
struct BatchItem<'item> {
    status: Option<&'item str>,
    error: Option<&'item str>,
}

impl<'item> BatchItem<'item> {
    fn parse(item: &'item serde_json::Value) -> Self {
        Self {
            status: item.get("status").and_then(serde_json::Value::as_str),
            error: item.get("error").and_then(serde_json::Value::as_str),
        }
    }

    fn error_message(self) -> Option<String> {
        if self.status == Some("error") {
            Some(self.error.unwrap_or("batch item failed").to_string())
        } else {
            None
        }
    }
}

/// Detect per-item errors in Qdrant batch endpoint responses.
///
/// Qdrant batch endpoints answer per item; a 200 response can still carry
/// per-item failures (`status: "error"`).
pub fn batch_item_error(item: &serde_json::Value) -> Option<String> {
    BatchItem::parse(item).error_message()
}

/// Verify that a batch response has the expected cardinality.
///
/// Returns `Ok(())` if `received == expected`, or a `QQL-BATCH-CARDINALITY` error otherwise.
pub fn verify_batch_cardinality(
    kind: &str,
    expected: usize,
    received: usize,
) -> Result<(), QqlError> {
    if expected == received {
        Ok(())
    } else {
        Err(QqlError::transport(
            "QQL-BATCH-CARDINALITY",
            alloc::format!("{kind} batch returned {received} results for {expected} operations"),
            None,
        ))
    }
}

/// Shared execution opts of a built batch, uniform across its members by
/// construction (the batch key guarantees it for ambient groups; explicit
/// `BATCH` blocks hoist them to the header, and the builders fail closed if
/// hand-built members disagree).
#[derive(Debug, Clone, PartialEq)]
pub struct BatchOpts {
    /// Shared read timeout in seconds (`?timeout=`); query batches only.
    pub timeout: Option<u64>,
    /// Shared read consistency (`?consistency=`); query batches only.
    pub consistency: Option<ReadConsistencyParam>,
    /// Effective `?wait=`; mutation batches only.
    pub wait: Option<bool>,
}

impl BatchOpts {
    fn for_query(timeout: Option<u64>, consistency: Option<ReadConsistencyParam>) -> Self {
        Self {
            timeout,
            consistency,
            wait: None,
        }
    }

    fn for_mutation(wait: bool) -> Self {
        Self {
            timeout: None,
            consistency: None,
            wait: Some(wait),
        }
    }
}

/// Build a `QueryBatchRequest` from a slice of `PlannedOperation` IRs.
///
/// Returns the shared read opts alongside the batch; members that disagree on
/// `timeout` / `consistency` fail with `QQL-BATCH-INVARIANT` instead of
/// silently collapsing onto one header value.
pub fn build_query_batch(
    operations: &[PlannedOperation],
) -> Result<(String, BatchOpts, QueryBatchRequest), QqlError> {
    if operations.is_empty() {
        return Err(QqlError::execution(
            "QQL-BATCH-INVARIANT",
            "cannot build query batch from empty operations",
            None,
        ));
    }
    let collection = operations[0].collection().unwrap_or_default().to_string();
    let mut opts: Option<BatchOpts> = None;
    let searches = operations
        .iter()
        .map(|operation| match operation {
            PlannedOperation::Query {
                request,
                collection: op_col,
            } => {
                if op_col != &collection {
                    return Err(QqlError::execution(
                        "QQL-BATCH-INVARIANT",
                        "query batch contained multiple collections",
                        None,
                    ));
                }
                let member_opts =
                    BatchOpts::for_query(request.timeout, request.consistency.clone());
                match &opts {
                    None => opts = Some(member_opts),
                    Some(first) if *first != member_opts => {
                        return Err(QqlError::execution(
                            "QQL-BATCH-INVARIANT",
                            "query batch members disagree on timeout / consistency",
                            None,
                        ));
                    }
                    Some(_) => {}
                }
                Ok(request.clone())
            }
            _ => Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "query batch contained a non-query operation",
                None,
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let opts = opts.expect("non-empty batch sets shared opts");
    Ok((collection, opts, QueryBatchRequest { searches }))
}

/// Moving variant of [`build_query_batch`]: destructures an owned operation
/// vector into the wire batch with zero clones.
///
/// Ambient executors own `Vec<PlannedOperation>` (via `BatchGrouper`), so the
/// hot success path moves each `QueryRequest` (vectors, filters, prefetches)
/// instead of cloning it. The borrowed [`build_query_batch`] stays for
/// callers that only borrow their members (explicit `BATCH` blocks, REST
/// projection, WASM error envelopes).
///
/// Build errors are fatal invariants (propagated with `?`, never retried), so
/// consuming the vector on error is safe: the caller returns `Err` directly.
pub fn into_query_batch(
    operations: Vec<PlannedOperation>,
) -> Result<(String, BatchOpts, QueryBatchRequest), QqlError> {
    if operations.is_empty() {
        return Err(QqlError::execution(
            "QQL-BATCH-INVARIANT",
            "cannot build query batch from empty operations",
            None,
        ));
    }
    let collection = operations[0].collection().unwrap_or_default().to_string();
    let mut opts: Option<BatchOpts> = None;
    let mut searches = Vec::with_capacity(operations.len());
    for operation in operations {
        match operation {
            PlannedOperation::Query {
                request,
                collection: op_col,
            } => {
                if op_col != collection {
                    return Err(QqlError::execution(
                        "QQL-BATCH-INVARIANT",
                        "query batch contained multiple collections",
                        None,
                    ));
                }
                let member_opts =
                    BatchOpts::for_query(request.timeout, request.consistency.clone());
                match &opts {
                    None => opts = Some(member_opts),
                    Some(first) if *first != member_opts => {
                        return Err(QqlError::execution(
                            "QQL-BATCH-INVARIANT",
                            "query batch members disagree on timeout / consistency",
                            None,
                        ));
                    }
                    Some(_) => {}
                }
                searches.push(request);
            }
            _ => {
                return Err(QqlError::execution(
                    "QQL-BATCH-INVARIANT",
                    "query batch contained a non-query operation",
                    None,
                ));
            }
        }
    }
    let opts = opts.expect("non-empty batch sets shared opts");
    Ok((collection, opts, QueryBatchRequest { searches }))
}

/// Build an `UpdateBatchRequest` from a slice of `PlannedOperation` IRs.
///
/// Returns the shared effective `wait` alongside the batch; members that
/// disagree fail with `QQL-BATCH-INVARIANT` instead of silently collapsing
/// onto one header value.
pub fn build_update_batch(
    operations: &[PlannedOperation],
) -> Result<(String, Vec<&'static str>, BatchOpts, UpdateBatchRequest), QqlError> {
    if operations.is_empty() {
        return Err(QqlError::execution(
            "QQL-BATCH-INVARIANT",
            "cannot build update batch from empty operations",
            None,
        ));
    }
    let mut updates = Vec::with_capacity(operations.len());
    let mut labels = Vec::with_capacity(operations.len());
    let mut collection = None;
    let mut wait: Option<bool> = None;

    for operation in operations {
        let Some(member_wait) = operation.mutation_wait() else {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "mutation batch contained a non-mutation operation",
                None,
            ));
        };
        match wait {
            None => wait = Some(member_wait),
            Some(first) if first != member_wait => {
                return Err(QqlError::execution(
                    "QQL-BATCH-INVARIANT",
                    "mutation batch members disagree on wait",
                    None,
                ));
            }
            Some(_) => {}
        }
        let Some((current_collection, update)) =
            crate::mutation::planned_to_update_operation(operation)
        else {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "mutation batch contained a non-mutation operation",
                None,
            ));
        };
        if collection
            .as_ref()
            .is_some_and(|col| col != &current_collection)
        {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "mutation batch contained multiple collections",
                None,
            ));
        }
        collection = Some(current_collection);
        labels.push(operation.operation_label());
        updates.push(update);
    }

    let collection = collection.unwrap_or_default();
    let wait = wait.expect("non-empty batch sets shared wait");
    Ok((
        collection,
        labels,
        BatchOpts::for_mutation(wait),
        UpdateBatchRequest {
            operations: updates,
        },
    ))
}

/// Moving variant of [`build_update_batch`]: destructures an owned operation
/// vector into the wire batch with zero clones.
///
/// Uses [`crate::mutation::planned_to_update_operation_owned`] so each
/// request (points, filters, payloads) moves instead of cloning. The borrowed
/// [`build_update_batch`] stays for callers that only borrow their members.
/// See [`into_query_batch`] for why consuming on error is safe.
pub fn into_update_batch(
    operations: Vec<PlannedOperation>,
) -> Result<(String, Vec<&'static str>, BatchOpts, UpdateBatchRequest), QqlError> {
    if operations.is_empty() {
        return Err(QqlError::execution(
            "QQL-BATCH-INVARIANT",
            "cannot build update batch from empty operations",
            None,
        ));
    }
    let mut updates = Vec::with_capacity(operations.len());
    let mut labels = Vec::with_capacity(operations.len());
    let mut collection = None;
    let mut wait: Option<bool> = None;

    for operation in operations {
        let label = operation.operation_label();
        let Some(member_wait) = operation.mutation_wait() else {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "mutation batch contained a non-mutation operation",
                None,
            ));
        };
        match wait {
            None => wait = Some(member_wait),
            Some(first) if first != member_wait => {
                return Err(QqlError::execution(
                    "QQL-BATCH-INVARIANT",
                    "mutation batch members disagree on wait",
                    None,
                ));
            }
            Some(_) => {}
        }
        let Some((current_collection, update)) =
            crate::mutation::planned_to_update_operation_owned(operation)
        else {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "mutation batch contained a non-mutation operation",
                None,
            ));
        };
        if collection
            .as_ref()
            .is_some_and(|col| col != &current_collection)
        {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "mutation batch contained multiple collections",
                None,
            ));
        }
        collection = Some(current_collection);
        labels.push(label);
        updates.push(update);
    }

    let collection = collection.unwrap_or_default();
    let wait = wait.expect("non-empty batch sets shared wait");
    Ok((
        collection,
        labels,
        BatchOpts::for_mutation(wait),
        UpdateBatchRequest {
            operations: updates,
        },
    ))
}

/// State machine orchestrating contiguous batch grouping across statements.
///
/// Shared between native Rust executor (`qql-runtime`) and WASM executor (`qql-wasm`).
#[derive(Debug, Default)]
pub struct BatchGrouper {
    pending: Vec<PlannedOperation>,
    pending_key: Option<BatchKey>,
}

impl BatchGrouper {
    /// Create a new empty batch grouper.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if an incoming raw AST statement violates the current batch family.
    ///
    /// A statement outside the current batch family is an execution barrier.
    /// Returns any pending batch that must be flushed *before* preparing `stmt`
    /// (because preparation may read or mutate backend state, e.g. UPSERT auto-creation).
    pub fn check_statement_barrier(&mut self, stmt: &Stmt) -> Option<Vec<PlannedOperation>> {
        let statement_key = statement_batch_key(stmt);
        if !self.pending.is_empty() && statement_key != self.pending_key {
            self.pending_key = None;
            Some(core::mem::take(&mut self.pending))
        } else {
            None
        }
    }

    /// Flush any pending batch when preparation or planning fails.
    pub fn flush_on_error(&mut self) -> Option<Vec<PlannedOperation>> {
        self.pending_key = None;
        if self.pending.is_empty() {
            None
        } else {
            Some(core::mem::take(&mut self.pending))
        }
    }

    /// Ingest a successfully planned operation.
    ///
    /// Returns `(flush_group, dispatch_single)`:
    /// - `flush_group`: Prior batch operations that must be flushed before this step.
    /// - `dispatch_single`: A non-batchable operation that must be dispatched individually.
    pub fn push_planned(
        &mut self,
        planned: PlannedOperation,
    ) -> (Option<Vec<PlannedOperation>>, Option<PlannedOperation>) {
        let key = planned.batch_key();
        if key.is_none() {
            self.pending_key = None;
            let flushed = if self.pending.is_empty() {
                None
            } else {
                Some(core::mem::take(&mut self.pending))
            };
            (flushed, Some(planned))
        } else if !self.pending.is_empty() && key != self.pending_key {
            let flushed = core::mem::take(&mut self.pending);
            self.pending_key = key;
            self.pending.push(planned);
            (Some(flushed), None)
        } else {
            self.pending_key = key;
            self.pending.push(planned);
            (None, None)
        }
    }

    /// Flush all remaining operations at the end of statement execution.
    pub fn finish(&mut self) -> Option<Vec<PlannedOperation>> {
        self.pending_key = None;
        if self.pending.is_empty() {
            None
        } else {
            Some(core::mem::take(&mut self.pending))
        }
    }

    /// Check if there are pending operations waiting to be flushed.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::plan;

    fn planned(source: &str) -> PlannedOperation {
        let stmt = qql_core::parser::Parser::parse(source).expect("test stmt must parse");
        plan(&stmt).expect("test stmt must plan")
    }

    #[test]
    fn into_query_batch_matches_borrowed_build() {
        let ops = alloc::vec![
            planned("QUERY [0.1, 0.2] FROM docs LIMIT 2;"),
            planned("QUERY [0.3, 0.4] FROM docs LIMIT 3;"),
        ];
        let (borrowed_collection, borrowed_opts, borrowed) =
            build_query_batch(&ops).expect("borrowed build must succeed");
        let (owned_collection, owned_opts, owned) =
            into_query_batch(ops).expect("owned build must succeed");
        assert_eq!(owned_collection, borrowed_collection);
        assert_eq!(owned_collection, "docs");
        assert_eq!(owned_opts, borrowed_opts);
        assert_eq!(owned_opts.timeout, None);
        assert_eq!(owned_opts.consistency, None);
        let borrowed_json = serde_json::to_value(&borrowed).expect("must serialize");
        let owned_json = serde_json::to_value(&owned).expect("must serialize");
        assert_eq!(owned_json, borrowed_json);
        assert_eq!(owned.searches.len(), 2);
    }

    #[test]
    fn into_update_batch_matches_borrowed_build() {
        let ops = alloc::vec![
            planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]} WAIT true;"),
            planned("DELETE PAYLOAD a FROM docs WHERE id = 1;"),
        ];
        let (borrowed_collection, borrowed_labels, borrowed_opts, borrowed) =
            build_update_batch(&ops).expect("borrowed build must succeed");
        let (owned_collection, owned_labels, owned_opts, owned) =
            into_update_batch(ops).expect("owned build must succeed");
        assert_eq!(owned_collection, borrowed_collection);
        assert_eq!(owned_labels, borrowed_labels);
        assert_eq!(owned_opts, borrowed_opts);
        let borrowed_json = serde_json::to_value(&borrowed).expect("must serialize");
        let owned_json = serde_json::to_value(&owned).expect("must serialize");
        assert_eq!(owned_json, borrowed_json);
    }

    #[test]
    fn query_batch_builders_carry_shared_read_opts() {
        let ops = alloc::vec![
            planned("QUERY [0.1, 0.2] FROM docs PARAMS (timeout = 5) LIMIT 2;"),
            planned("QUERY [0.3, 0.4] FROM docs PARAMS (timeout = 5) LIMIT 3;"),
        ];
        let (_, opts, _) = into_query_batch(ops).expect("uniform opts must build");
        assert_eq!(opts.timeout, Some(5));
        assert_eq!(opts.consistency, None);
    }

    #[test]
    fn query_batch_builders_reject_mixed_read_opts() {
        let ops = alloc::vec![
            planned("QUERY [0.1, 0.2] FROM docs PARAMS (timeout = 5) LIMIT 2;"),
            planned("QUERY [0.3, 0.4] FROM docs PARAMS (timeout = 30) LIMIT 3;"),
        ];
        let err = build_query_batch(&ops).expect_err("mixed opts must not build");
        assert_eq!(err.code, "QQL-BATCH-INVARIANT");
        assert!(err.message.contains("timeout"), "{err:?}");
    }

    #[test]
    fn mutation_batch_builders_reject_mixed_wait() {
        let ops = alloc::vec![
            planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]} WAIT false;"),
            planned("DELETE FROM docs WHERE id = 1;"),
        ];
        let err = build_update_batch(&ops).expect_err("mixed wait must not build");
        assert_eq!(err.code, "QQL-BATCH-INVARIANT");
        assert!(err.message.contains("wait"), "{err:?}");
    }

    #[test]
    fn statement_keys_split_on_read_opts_and_wait() {
        let timed_5 =
            qql_core::parser::Parser::parse("QUERY [0.1] FROM docs PARAMS (timeout = 5);").unwrap();
        let timed_30 =
            qql_core::parser::Parser::parse("QUERY [0.1] FROM docs PARAMS (timeout = 30);")
                .unwrap();
        let timed_all = qql_core::parser::Parser::parse(
            "QUERY [0.1] FROM docs PARAMS (timeout = 30, consistency = all);",
        )
        .unwrap();
        let untimed = qql_core::parser::Parser::parse("QUERY [0.1] FROM docs;").unwrap();
        assert_eq!(statement_batch_key(&timed_5), statement_batch_key(&timed_5));
        assert_ne!(
            statement_batch_key(&timed_5),
            statement_batch_key(&timed_30)
        );
        assert_ne!(
            statement_batch_key(&timed_30),
            statement_batch_key(&timed_all)
        );
        assert_ne!(statement_batch_key(&timed_5), statement_batch_key(&untimed));
        assert_eq!(
            statement_batch_key(&timed_all),
            Some(BatchKey::Query {
                collection: "docs".into(),
                timeout: Some(30),
                consistency: Some(ReadConsistencyParam::All),
            })
        );

        let wait_false = qql_core::parser::Parser::parse(
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT false;",
        )
        .unwrap();
        let wait_false_twin = qql_core::parser::Parser::parse(
            "UPSERT INTO docs VALUES {id: 2, vector: [0.1]} WAIT false;",
        )
        .unwrap();
        let wait_true = qql_core::parser::Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
        assert_eq!(
            statement_batch_key(&wait_false),
            statement_batch_key(&wait_false_twin)
        );
        assert_ne!(
            statement_batch_key(&wait_false),
            statement_batch_key(&wait_true)
        );
    }

    #[test]
    fn ambient_grouper_splits_differing_read_opts_and_waits() {
        // Differing timeout/consistency must never co-group: each group
        // executes with its own opts.
        let mut grouper = BatchGrouper::new();
        assert!(
            grouper
                .push_planned(planned(
                    "QUERY [0.1] FROM docs PARAMS (timeout = 5) LIMIT 2;"
                ))
                .1
                .is_none()
        );
        let (flush, single) = grouper.push_planned(planned(
            "QUERY [0.2] FROM docs PARAMS (timeout = 30, consistency = all) LIMIT 3;",
        ));
        assert!(single.is_none());
        assert_eq!(flush.expect("opts differ -> flush").len(), 1);
        let pending = grouper.finish().expect("pending group");
        let (_, opts, batch) = into_query_batch(pending).expect("group builds");
        assert_eq!(opts.timeout, Some(30));
        assert_eq!(opts.consistency, Some(ReadConsistencyParam::All));
        assert_eq!(batch.searches.len(), 1);
    }

    #[test]
    fn ambient_grouper_keeps_uniform_wait_together() {
        let mut grouper = BatchGrouper::new();
        assert!(
            grouper
                .push_planned(planned(
                    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT false;"
                ))
                .1
                .is_none()
        );
        let (flush, single) = grouper.push_planned(planned(
            "UPSERT INTO docs VALUES {id: 2, vector: [0.1]} WAIT false;",
        ));
        assert!(flush.is_none(), "same wait must co-group");
        assert!(single.is_none());
        let pending = grouper.finish().expect("pending group");
        assert_eq!(pending.len(), 2);
        let (_, _, opts, batch) = into_update_batch(pending).expect("group builds");
        assert_eq!(opts.wait, Some(false));
        assert_eq!(batch.operations.len(), 2);

        // A `WAIT true` neighbor is a different group.
        let mut grouper = BatchGrouper::new();
        assert!(
            grouper
                .push_planned(planned(
                    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT false;"
                ))
                .1
                .is_none()
        );
        let (flush, single) = grouper.push_planned(planned(
            "UPSERT INTO docs VALUES {id: 2, vector: [0.1]} WAIT true;",
        ));
        assert!(single.is_none());
        assert_eq!(flush.expect("wait differs -> flush").len(), 1);
        assert_eq!(grouper.finish().expect("pending group").len(), 1);
    }

    #[test]
    fn into_batches_reject_empty_and_heterogeneous() {
        assert!(into_query_batch(Vec::new()).is_err());
        assert!(into_update_batch(Vec::new()).is_err());
        // Query builder rejects mutations and vice versa.
        let mutation = planned("DELETE PAYLOAD a FROM docs WHERE id = 1;");
        assert!(into_query_batch(alloc::vec![mutation]).is_err());
        let query = planned("QUERY [0.1, 0.2] FROM docs LIMIT 1;");
        assert!(into_update_batch(alloc::vec![query]).is_err());
    }

    #[test]
    fn into_batches_reject_multiple_collections() {
        let q1 = planned("QUERY [0.1, 0.2] FROM col_a LIMIT 1;");
        let q2 = planned("QUERY [0.1, 0.2] FROM col_b LIMIT 1;");
        assert!(into_query_batch(alloc::vec![q1.clone(), q2.clone()]).is_err());
        assert!(build_query_batch(&[q1, q2]).is_err());

        let m1 = planned("UPSERT INTO col_a VALUES {id: 1, vector: [0.1, 0.2]};");
        let m2 = planned("UPSERT INTO col_b VALUES {id: 2, vector: [0.1, 0.2]};");
        assert!(into_update_batch(alloc::vec![m1.clone(), m2.clone()]).is_err());
        assert!(build_update_batch(&[m1, m2]).is_err());
    }

    #[test]
    fn update_operation_owned_roundtrip_preserves_variant_and_collection() {
        let ops = alloc::vec![
            planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]} WAIT true;"),
            planned("DELETE FROM docs WHERE id = 1;"),
            planned("UPDATE docs SET PAYLOAD = {a: 1} WHERE id = 1;"),
            planned("UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1;"),
            planned("CLEAR PAYLOAD FROM docs WHERE id = 1;"),
            planned("DELETE PAYLOAD a FROM docs WHERE id = 1;"),
            planned("UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;"),
            planned("DELETE VECTOR default FROM docs WHERE id = 1;"),
        ];
        assert_eq!(ops.len(), 8);
        let (collection, labels, opts, batch) = into_update_batch(ops).expect("must build");
        assert_eq!(labels.len(), batch.operations.len());
        assert_eq!(labels.len(), 8);
        assert_eq!(opts.wait, Some(true));
        for (label, wire_op) in labels.iter().zip(batch.operations) {
            let rebuilt =
                crate::mutation::update_operation_into_planned(&collection, wire_op, true);
            assert_eq!(rebuilt.collection().unwrap(), collection);
            assert_eq!(&rebuilt.operation_label(), label);
        }
    }
}
