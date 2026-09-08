//! Batch grouping, extraction, and validation for multi-statement execution.

use crate::plan::PlannedOperation;
use crate::types::{QueryBatchRequest, UpdateBatchRequest};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use qql_core::ast::{QueryCollection, QueryExpr, Stmt};
use qql_core::error::QqlError;

/// Batch grouping key identifying contiguous operations that can be combined
/// into a single Qdrant batch RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchKey {
    /// Query batch for the named collection.
    Query(String),
    /// Mutation batch for the named collection.
    Mutation(String),
}

/// Batch grouping key for a raw AST statement (before preparation/planning).
///
/// Returns `None` for statements that are never batchable (DDL, SHOW, group
/// queries, point-ID lookups, etc.).
pub fn statement_batch_key(stmt: &Stmt) -> Option<BatchKey> {
    match stmt {
        Stmt::Query(query)
            if query.group.is_none()
                && !matches!(query.expression, QueryExpr::Points { .. })
                && !matches!(query.expression, QueryExpr::CrossRerank { .. }) =>
        {
            match &query.collection {
                QueryCollection::Explicit(collection) => Some(BatchKey::Query(collection.clone())),
                QueryCollection::Inherited => None,
            }
        }
        Stmt::Upsert(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::Delete(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::UpdatePayload(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::ClearPayload(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::DeletePayload(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::UpdateVector(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::DeleteVector(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        _ => None,
    }
}

/// Detect per-item errors in Qdrant batch endpoint responses.
///
/// Qdrant batch endpoints answer per item; a 200 response can still carry
/// per-item failures (`status: "error"`).
pub fn batch_item_error(item: &serde_json::Value) -> Option<String> {
    if item.get("status").and_then(serde_json::Value::as_str) == Some("error") {
        return Some(
            item.get("error")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    item.pointer("/status/error")
                        .and_then(serde_json::Value::as_str)
                })
                .unwrap_or("batch item failed")
                .to_string(),
        );
    }
    None
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

/// Build a `QueryBatchRequest` from a slice of `PlannedOperation` IRs.
pub fn build_query_batch(
    operations: &[PlannedOperation],
) -> Result<(String, QueryBatchRequest), QqlError> {
    if operations.is_empty() {
        return Err(QqlError::execution(
            "QQL-BATCH-INVARIANT",
            "cannot build query batch from empty operations",
            None,
        ));
    }
    let collection = operations[0].collection().unwrap_or_default().to_string();
    let searches = operations
        .iter()
        .map(|operation| match operation {
            PlannedOperation::Query { request, .. } => Ok(request.clone()),
            _ => Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "query batch contained a non-query operation",
                None,
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((collection, QueryBatchRequest { searches }))
}

/// Build an `UpdateBatchRequest` from a slice of `PlannedOperation` IRs.
pub fn build_update_batch(
    operations: &[PlannedOperation],
) -> Result<(String, Vec<&'static str>, UpdateBatchRequest), QqlError> {
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

    for operation in operations {
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
    Ok((
        collection,
        labels,
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
