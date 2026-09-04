//! Canonical fallible planner: AST → [`PlannedOperation`].
//!
//! `PlannedOperation` is the transport-neutral source of truth. REST routes
//! are a projection (`to_rest_route`). gRPC converts the same typed operation.

use crate::ddl::{lower_alter_collection, lower_create_collection, lower_create_index};
use crate::mutation::{
    lower_clear_payload_request, lower_delete_payload_request, lower_delete_request,
    lower_delete_vector_request, lower_scroll_request, lower_update_payload_request,
    lower_update_vector_request, lower_upsert_request,
};
use crate::query::{lower_query_groups_request, lower_query_request};
use crate::routing::Route;
use crate::types::*;
use qql_core::ast::{
    QueryCollection, QueryExpr, QueryInput, Stmt, VectorKind, VectorTarget, VectorValue,
};
use qql_core::error::QqlError;

/// Full frontend validity gate: parse a script and plan-validate every
/// statement.
///
/// This is the exact contract the language conformance suite enforces
/// (`parse_all` + `plan`) and the runtime applies before executing, so
/// `is_ok()` here means the script would be accepted for execution. Bindings
/// (`pyqql` / `nqql` / `qql-wasm`) expose this as their `is_valid`.
pub fn parse_and_plan(source: &str) -> Result<Vec<Stmt>, QqlError> {
    let statements = qql_core::parser::Parser::parse_all(source)?;
    for statement in &statements {
        plan(statement)?;
    }
    Ok(statements)
}

/// Canonical planned operation. Batch compatibility is determined from this
/// type, not from raw AST.
#[derive(Debug, Clone)]
pub enum PlannedOperation {
    /// Vector or text search: `POST /collections/{c}/points/query`.
    Query {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/query` request body.
        request: QueryRequest,
    },
    /// Grouped search: `POST /collections/{c}/points/query/groups`.
    QueryGroups {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/query/groups` request body.
        request: QueryGroupsRequest,
    },
    /// Point-ID retrieval: `POST /collections/{c}/points`.
    GetPoints {
        /// Target collection name.
        collection: String,
        /// Lowered `/points` request body.
        request: PointsRequest,
    },
    /// Keyset pagination: `POST /collections/{c}/points/scroll`.
    Scroll {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/scroll` request body.
        request: ScrollRequest,
    },
    /// Count matching points: `POST /collections/{c}/points/count`.
    Count {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/count` request body.
        request: CountRequest,
    },
    /// In-database facet aggregation (`POST /collections/{collection}/facet`).
    Facet {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/facet` request body.
        request: FacetRequest,
    },
    /// Point upsert: `PUT /collections/{c}/points`.
    Upsert {
        /// Target collection name.
        collection: String,
        /// Lowered `/points` upsert request body.
        request: UpsertRequest,
        /// Force `wait=true` on the route when embedding resolution runs.
        wait: bool,
    },
    /// Point deletion by ID or filter: `POST /collections/{c}/points/delete`.
    Delete {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/delete` request body.
        request: DeleteRequest,
    },
    /// Merge payload keys: `POST /collections/{c}/points/payload`.
    UpdatePayload {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/payload` request body.
        request: UpdatePayloadRequest,
    },
    /// Drop all payload: `POST /collections/{c}/points/payload/clear`.
    ClearPayload {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/payload/clear` request body.
        request: ClearPayloadRequest,
    },
    /// Remove payload keys: `POST /collections/{c}/points/payload/delete`.
    DeletePayload {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/payload/delete` request body.
        request: DeletePayloadRequest,
    },
    /// Replace point vectors: `PUT /collections/{c}/points/vectors`.
    UpdateVectors {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/vectors` request body.
        request: UpdateVectorRequest,
    },
    /// Remove named vectors: `POST /collections/{c}/points/vectors/delete`.
    DeleteVectors {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/vectors/delete` request body.
        request: DeleteVectorRequest,
    },
    /// Create a collection: `PUT /collections/{c}`.
    CreateCollection {
        /// Target collection name.
        collection: String,
        /// Lowered create-collection request body.
        request: CreateCollectionRequest,
    },
    /// Alter a collection: `PATCH /collections/{c}`.
    UpdateCollection {
        /// Target collection name.
        collection: String,
        /// Lowered alter-collection request body.
        request: UpdateCollectionRequest,
    },
    /// Drop a collection: `DELETE /collections/{c}`.
    DropCollection {
        /// Target collection name.
        collection: String,
    },
    /// Create a payload index: `PUT /collections/{c}/index`.
    CreateIndex {
        /// Target collection name.
        collection: String,
        /// Lowered create-index request body.
        request: CreateIndexRequest,
    },
    /// Drop a payload index: `DELETE /collections/{c}/index/{field}`.
    DropIndex {
        /// Target collection name.
        collection: String,
        /// Payload field whose index is dropped.
        field: String,
    },
    /// Create a custom shard key: `PUT /collections/{c}/shards`.
    CreateShardKey {
        /// Target collection name.
        collection: String,
        /// Lowered create-shard-key request body.
        request: CreateShardKeyRequest,
    },
    /// Drop a custom shard key: `POST /collections/{c}/shards/delete`.
    DropShardKey {
        /// Target collection name.
        collection: String,
        /// Lowered drop-shard-key request body.
        request: DropShardKeyRequest,
    },
    /// List custom shard keys: `GET /collections/{c}/shards`.
    ListShardKeys {
        /// Target collection name.
        collection: String,
    },
    /// List collections: `GET /collections`.
    ListCollections,
    /// Inspect a collection: `GET /collections/{c}`.
    GetCollection {
        /// Target collection name.
        collection: String,
    },
    /// Client-side cross-encoder: run candidate queries, score pairs, reorder.
    CrossRerank {
        /// Target collection name.
        collection: String,
        /// Natural-language query text for the cross-encoder.
        query: String,
        /// Cross-encoder model identifier.
        model: String,
        /// Payload field holding document text for pair scoring.
        field: String,
        /// Final result limit after reranking.
        limit: u64,
        /// Result offset after reranking.
        offset: u64,
        /// Candidate ANN stages already planned as normal queries.
        candidates: Vec<(String, QueryRequest)>,
    },
    /// Show the cluster-wide resource quota config and current utilization.
    GetQuotas,
    /// Replace the cluster-wide resource quota configuration.
    SetQuotas {
        /// Replacement quota config; omitted keys become uncapped defaults.
        request: SetQuotaRequest,
    },
}

impl PlannedOperation {
    /// Human-readable label for executor responses.
    pub fn operation_label(&self) -> &'static str {
        match self {
            PlannedOperation::Query { .. } => "QUERY",
            PlannedOperation::QueryGroups { .. } => "QUERY_GROUPS",
            PlannedOperation::GetPoints { .. } => "GET_POINTS",
            PlannedOperation::Scroll { .. } => "SCROLL",
            PlannedOperation::Count { .. } => "COUNT",
            PlannedOperation::Facet { .. } => "FACET",
            PlannedOperation::Upsert { .. } => "UPSERT",
            PlannedOperation::Delete { .. } => "DELETE",
            PlannedOperation::UpdatePayload { .. } => "UPDATE_PAYLOAD",
            PlannedOperation::ClearPayload { .. } => "CLEAR_PAYLOAD",
            PlannedOperation::DeletePayload { .. } => "DELETE_PAYLOAD",
            PlannedOperation::UpdateVectors { .. } => "UPDATE_VECTOR",
            PlannedOperation::DeleteVectors { .. } => "DELETE_VECTOR",
            PlannedOperation::CreateCollection { .. } => "CREATE_COLLECTION",
            PlannedOperation::UpdateCollection { .. } => "ALTER_COLLECTION",
            PlannedOperation::DropCollection { .. } => "DROP_COLLECTION",
            PlannedOperation::CreateIndex { .. } => "CREATE_INDEX",
            PlannedOperation::DropIndex { .. } => "DROP_INDEX",
            PlannedOperation::CreateShardKey { .. } => "CREATE_SHARD_KEY",
            PlannedOperation::DropShardKey { .. } => "DROP_SHARD_KEY",
            PlannedOperation::ListShardKeys { .. } => "SHOW_SHARD_KEYS",
            PlannedOperation::ListCollections => "SHOW_COLLECTIONS",
            PlannedOperation::GetCollection { .. } => "SHOW_COLLECTION",
            PlannedOperation::CrossRerank { .. } => "CROSS_RERANK",
            PlannedOperation::GetQuotas => "SHOW_QUOTAS",
            PlannedOperation::SetQuotas { .. } => "SET_QUOTA",
        }
    }

    /// Stable snake_case type id for SDK `compile()` / route metadata.
    ///
    /// Prefer this over inferring type from REST method+path (body-less routes
    /// like DROP INDEX and SHOW SHARD KEYS share method+shape with other ops).
    pub fn compile_stmt_type(&self) -> &'static str {
        match self {
            PlannedOperation::Query { .. } => "query",
            PlannedOperation::QueryGroups { .. } => "query_groups",
            PlannedOperation::GetPoints { .. } => "points",
            PlannedOperation::Scroll { .. } => "scroll",
            PlannedOperation::Count { .. } => "count",
            PlannedOperation::Facet { .. } => "facet",
            PlannedOperation::Upsert { .. } => "upsert",
            PlannedOperation::Delete { .. } => "delete",
            PlannedOperation::UpdatePayload { .. } => "update_payload",
            PlannedOperation::ClearPayload { .. } => "clear_payload",
            PlannedOperation::DeletePayload { .. } => "delete_payload",
            PlannedOperation::UpdateVectors { .. } => "update_vector",
            PlannedOperation::DeleteVectors { .. } => "delete_vector",
            PlannedOperation::CreateCollection { .. } => "create_collection",
            PlannedOperation::UpdateCollection { .. } => "update_collection",
            PlannedOperation::DropCollection { .. } => "drop_collection",
            PlannedOperation::CreateIndex { .. } => "create_index",
            PlannedOperation::DropIndex { .. } => "drop_index",
            PlannedOperation::CreateShardKey { .. } => "create_shard_key",
            PlannedOperation::DropShardKey { .. } => "drop_shard_key",
            PlannedOperation::ListShardKeys { .. } => "show_shard_keys",
            PlannedOperation::ListCollections => "show_collections",
            PlannedOperation::GetCollection { .. } => "show_collection",
            PlannedOperation::CrossRerank { .. } => "cross_rerank",
            PlannedOperation::GetQuotas => "show_quotas",
            PlannedOperation::SetQuotas { .. } => "set_quota",
        }
    }

    /// Collection targeted by this operation, when applicable.
    pub fn collection(&self) -> Option<&str> {
        match self {
            PlannedOperation::Query { collection, .. }
            | PlannedOperation::QueryGroups { collection, .. }
            | PlannedOperation::GetPoints { collection, .. }
            | PlannedOperation::Scroll { collection, .. }
            | PlannedOperation::Count { collection, .. }
            | PlannedOperation::Facet { collection, .. }
            | PlannedOperation::Upsert { collection, .. }
            | PlannedOperation::Delete { collection, .. }
            | PlannedOperation::UpdatePayload { collection, .. }
            | PlannedOperation::ClearPayload { collection, .. }
            | PlannedOperation::DeletePayload { collection, .. }
            | PlannedOperation::UpdateVectors { collection, .. }
            | PlannedOperation::DeleteVectors { collection, .. }
            | PlannedOperation::CreateCollection { collection, .. }
            | PlannedOperation::UpdateCollection { collection, .. }
            | PlannedOperation::DropCollection { collection }
            | PlannedOperation::CreateIndex { collection, .. }
            | PlannedOperation::DropIndex { collection, .. }
            | PlannedOperation::CreateShardKey { collection, .. }
            | PlannedOperation::DropShardKey { collection, .. }
            | PlannedOperation::ListShardKeys { collection }
            | PlannedOperation::GetCollection { collection }
            | PlannedOperation::CrossRerank { collection, .. } => Some(collection.as_str()),
            PlannedOperation::ListCollections
            | PlannedOperation::GetQuotas
            | PlannedOperation::SetQuotas { .. } => None,
        }
    }

    /// Batch family for smart batching of adjacent operations.
    pub fn batch_family(&self) -> BatchFamily {
        match self {
            PlannedOperation::Query { .. } => BatchFamily::Query,
            PlannedOperation::Upsert { .. }
            | PlannedOperation::Delete { .. }
            | PlannedOperation::UpdatePayload { .. }
            | PlannedOperation::ClearPayload { .. }
            | PlannedOperation::DeletePayload { .. }
            | PlannedOperation::UpdateVectors { .. }
            | PlannedOperation::DeleteVectors { .. } => BatchFamily::Mutation,
            // Pair scoring is not batchable with plain queries.
            PlannedOperation::CrossRerank { .. } => BatchFamily::Single,
            _ => BatchFamily::Single,
        }
    }

    /// Batch grouping key (collection + family) for executor dispatch.
    ///
    /// Returns `None` for single-shot operations that cannot be grouped.
    pub fn batch_key(&self) -> Option<BatchKey> {
        match self.batch_family() {
            BatchFamily::Query => match self {
                PlannedOperation::Query { collection, .. } => {
                    Some(BatchKey::Query(collection.clone()))
                }
                _ => None,
            },
            BatchFamily::Mutation => self
                .collection()
                .map(|collection| BatchKey::Mutation(collection.to_owned())),
            BatchFamily::Single => None,
        }
    }

    /// Shard key carried on the plan, when present.
    pub fn shard_key(&self) -> Option<&str> {
        match self {
            PlannedOperation::Query { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::QueryGroups { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::GetPoints { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::Scroll { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::Count { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::Facet { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::Upsert { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::Delete { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::UpdatePayload { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::ClearPayload { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::DeletePayload { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::UpdateVectors { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::DeleteVectors { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::CreateShardKey { request, .. } => Some(request.shard_key.as_str()),
            PlannedOperation::DropShardKey { request, .. } => Some(request.shard_key.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Classification family for executor batch sharing: which statements may run
/// together as one contiguous same-collection batch.
pub enum BatchFamily {
    /// Joins a query batch (`/points/query/batch`).
    Query,
    /// Joins a mutation batch (`UpdateOperations` / `UpdateBatchPoints`).
    Mutation,
    /// Always executed alone.
    Single,
}

/// Grouping key for statement/operation batching (same collection + family).
///
/// Used by executors to collect adjacent operations into query/mutation
/// batches before flushing to the backend.
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
            if query.group.is_none() && !matches!(query.expression, QueryExpr::Points { .. }) =>
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

/// Fallible planner — the single source of truth for statement → operation.
pub fn plan(statement: &Stmt) -> Result<PlannedOperation, QqlError> {
    match statement {
        Stmt::Query(query) => {
            validate_query_stmt(query)?;
            let collection = match &query.collection {
                QueryCollection::Explicit(name) if !name.is_empty() => name.clone(),
                QueryCollection::Explicit(_) => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION",
                        "query collection name must not be empty",
                        None,
                    ));
                }
                QueryCollection::Inherited => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION",
                        "top-level query requires an explicit collection (FROM ...)",
                        None,
                    ));
                }
            };

            if matches!(query.expression, QueryExpr::Points { .. }) {
                let ids = match &query.expression {
                    QueryExpr::Points { ids } => {
                        ids.iter().map(crate::semantic::PlanPointId::from).collect()
                    }
                    _ => unreachable!(),
                };
                let (with_payload, with_vector) =
                    crate::query::lower_output_selector_public(&query.output);
                return Ok(PlannedOperation::GetPoints {
                    collection,
                    request: PointsRequest {
                        ids,
                        with_payload,
                        with_vector,
                        shard_key: query.shard_key.clone(),
                    },
                });
            }

            if let QueryExpr::CrossRerank {
                query: qtext,
                model,
                field,
                prefetch,
            } = &query.expression
            {
                return plan_cross_rerank(query, &collection, qtext, model, field, prefetch);
            }

            if query.group.is_some() {
                // GROUP BY is routed to QueryGroups which supports both LIMIT and OFFSET (via group_offset).
                return Ok(PlannedOperation::QueryGroups {
                    collection,
                    request: lower_query_groups_request(query)?,
                });
            }

            Ok(PlannedOperation::Query {
                collection,
                request: lower_query_request(query)?,
            })
        }
        Stmt::Scroll(scroll) => Ok(PlannedOperation::Scroll {
            collection: scroll.collection.clone(),
            request: lower_scroll_request(
                scroll.limit,
                scroll.filter.as_deref(),
                scroll.after.as_ref(),
                scroll.shard_key.clone(),
                scroll.with_vector.as_ref(),
            ),
        }),
        Stmt::Upsert(upsert) => Ok(PlannedOperation::Upsert {
            collection: upsert.collection.clone(),
            request: lower_upsert_request(upsert),
            wait: upsert.embedding.is_some() || !upsert.embed.is_empty(),
        }),
        Stmt::Delete(delete) => Ok(PlannedOperation::Delete {
            collection: delete.collection.clone(),
            request: lower_delete_request(delete),
        }),
        Stmt::ClearPayload(clear) => Ok(PlannedOperation::ClearPayload {
            collection: clear.collection.clone(),
            request: lower_clear_payload_request(clear),
        }),
        Stmt::DeletePayload(del) => Ok(PlannedOperation::DeletePayload {
            collection: del.collection.clone(),
            request: lower_delete_payload_request(del),
        }),
        Stmt::DeleteVector(del_vec) => Ok(PlannedOperation::DeleteVectors {
            collection: del_vec.collection.clone(),
            request: lower_delete_vector_request(del_vec),
        }),
        Stmt::UpdateVector(update) => Ok(PlannedOperation::UpdateVectors {
            collection: update.collection.clone(),
            request: lower_update_vector_request(update),
        }),
        Stmt::UpdatePayload(update) => Ok(PlannedOperation::UpdatePayload {
            collection: update.collection.clone(),
            request: lower_update_payload_request(update),
        }),
        Stmt::CreateCollection(create) => Ok(PlannedOperation::CreateCollection {
            collection: create.collection.clone(),
            request: lower_create_collection(create),
        }),
        Stmt::AlterCollection(alter) => Ok(PlannedOperation::UpdateCollection {
            collection: alter.collection.clone(),
            request: lower_alter_collection(alter),
        }),
        Stmt::DropCollection(drop) => Ok(PlannedOperation::DropCollection {
            collection: drop.collection.clone(),
        }),
        Stmt::CreateIndex(index) => Ok(PlannedOperation::CreateIndex {
            collection: index.collection.clone(),
            request: lower_create_index(index),
        }),
        Stmt::DropIndex(index) => Ok(PlannedOperation::DropIndex {
            collection: index.collection.clone(),
            field: index.field.clone(),
        }),
        Stmt::Count(count) => {
            let collection = match &count.collection {
                qql_core::ast::QueryCollection::Explicit(name) if !name.is_empty() => name.clone(),
                qql_core::ast::QueryCollection::Explicit(_) => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION",
                        "count collection name must not be empty",
                        None,
                    ));
                }
                qql_core::ast::QueryCollection::Inherited => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION",
                        "count requires an explicit collection (FROM ...)",
                        None,
                    ));
                }
            };
            // Filter and shard routing are independent: filter → qdrant.Filter,
            // shard_key → request ShardKeySelector (gRPC) / shard_key (REST).
            let filter = count
                .filter
                .as_ref()
                .map(|f| crate::filter::top_level_filter(f));
            Ok(PlannedOperation::Count {
                collection,
                request: CountRequest {
                    filter,
                    shard_key: count.shard_key.clone(),
                    exact: count.exact,
                },
            })
        }
        Stmt::Facet(facet) => {
            let collection = match &facet.collection {
                qql_core::ast::QueryCollection::Explicit(name) if !name.is_empty() => name.clone(),
                qql_core::ast::QueryCollection::Explicit(_) => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION",
                        "facet collection name must not be empty",
                        None,
                    ));
                }
                qql_core::ast::QueryCollection::Inherited => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION",
                        "facet requires an explicit collection (FROM ...)",
                        None,
                    ));
                }
            };
            let filter = facet
                .filter
                .as_ref()
                .map(|f| crate::filter::top_level_filter(f));
            let limit = match facet.limit {
                Some(n) if n > usize::MAX as u64 => {
                    return Err(QqlError::validation(
                        "QQL-VALIDATION-LIMIT-OVERFLOW",
                        alloc::format!("facet limit {n} exceeds platform usize::MAX"),
                        None,
                    ));
                }
                Some(n) => Some(n as usize),
                None => None,
            };
            Ok(PlannedOperation::Facet {
                collection,
                request: FacetRequest {
                    key: facet.key.clone(),
                    limit,
                    filter,
                    exact: facet.exact,
                    shard_key: facet.shard_key.clone(),
                },
            })
        }
        Stmt::CreateShardKey(sk) => Ok(PlannedOperation::CreateShardKey {
            collection: sk.collection.clone(),
            request: CreateShardKeyRequest {
                shard_key: sk.shard_key.clone(),
                shards_number: sk.shards_number,
                replication_factor: sk.replication_factor,
            },
        }),
        Stmt::DropShardKey(sk) => Ok(PlannedOperation::DropShardKey {
            collection: sk.collection.clone(),
            request: DropShardKeyRequest {
                shard_key: sk.shard_key.clone(),
            },
        }),
        Stmt::ShowCollections => Ok(PlannedOperation::ListCollections),
        Stmt::ShowCollection(collection) => Ok(PlannedOperation::GetCollection {
            collection: collection.clone(),
        }),
        Stmt::ShowShardKeys(collection) => Ok(PlannedOperation::ListShardKeys {
            collection: collection.clone(),
        }),
        Stmt::ShowQuotas => Ok(PlannedOperation::GetQuotas),
        Stmt::SetQuota(stmt) => Ok(PlannedOperation::SetQuotas {
            request: lower_set_quota(stmt)?,
        }),
    }
}

/// Lower a `SET QUOTA (…)` statement to a typed quota request.
///
/// `PUT /quotas` **replaces** the entire cluster-wide config. Omitted keys
/// (including `key = null`) are not set on the replacement body, so they
/// become "uncapped / default" in the new config — not a patch of the old
/// one. Callers that want to keep existing limits must restate them.
fn lower_set_quota(stmt: &qql_core::ast::SetQuotaStmt) -> Result<SetQuotaRequest, QqlError> {
    use qql_core::ast::Value;

    let mut request = SetQuotaRequest {
        enabled: None,
        max_resident_memory_percent: None,
        max_disk_usage_percent: None,
        release_margin_percent: None,
        wait: stmt.wait,
    };
    for (key, value) in &stmt.config {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "enabled" => match value {
                Value::Bool(b) => request.enabled = Some(*b),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-QUOTA",
                        "enabled must be true or false",
                        None,
                    ));
                }
            },
            "max_resident_memory_percent" | "max_disk_usage_percent" => {
                request = apply_quota_percent(request, &lower, value, 1, 100)?;
            }
            "release_margin_percent" => {
                request = apply_quota_percent(request, &lower, value, 0, 100)?;
            }
            _ => {
                return Err(QqlError::validation(
                    "QQL-PLAN-QUOTA",
                    format!(
                        "unknown quota parameter '{key}'. Expected: enabled, max_resident_memory_percent, max_disk_usage_percent, release_margin_percent"
                    ),
                    None,
                ));
            }
        }
    }
    Ok(request)
}

fn apply_quota_percent(
    mut request: SetQuotaRequest,
    key: &str,
    value: &qql_core::ast::Value,
    min: u64,
    max: u64,
) -> Result<SetQuotaRequest, QqlError> {
    match value {
        // Explicit null → leave field unset so the replacement config has no
        // cap for this resource (full PUT replace semantics).
        qql_core::ast::Value::Null => {}
        qql_core::ast::Value::Int(n) if *n >= min as i64 && (*n as u64) <= max => {
            let n = *n as u64;
            match key {
                "max_resident_memory_percent" => request.max_resident_memory_percent = Some(n),
                "max_disk_usage_percent" => request.max_disk_usage_percent = Some(n),
                _ => request.release_margin_percent = Some(n),
            }
        }
        _ => {
            return Err(QqlError::validation(
                "QQL-PLAN-QUOTA",
                format!("{key} must be an integer in [{min}, {max}] or null"),
                None,
            ));
        }
    }
    Ok(request)
}

fn validate_query_stmt(query: &qql_core::ast::QueryStmt) -> Result<(), QqlError> {
    for cte in &query.ctes {
        validate_query_stmt(&cte.query)?;
    }
    validate_query_expr(&query.expression)?;

    let has_rrf_params = query
        .params
        .as_ref()
        .is_some_and(|params| params.rrf_k.is_some() || params.rrf_weights.is_some());
    let accepts_rrf_params = matches!(
        &query.expression,
        QueryExpr::Fusion {
            method: qql_core::ast::FusionMethod::Rrf,
            ..
        } | QueryExpr::Hybrid {
            fusion: qql_core::ast::FusionMethod::Rrf,
            ..
        }
    );
    if has_rrf_params && !accepts_rrf_params {
        return Err(QqlError::validation(
            "QQL-PLAN-RRF-PARAMS",
            "rrf_k and rrf_weights are valid only with RRF fusion",
            None,
        ));
    }
    if let Some(weights) = query
        .params
        .as_ref()
        .and_then(|params| params.rrf_weights.as_ref())
    {
        let prefetch_count = match &query.expression {
            QueryExpr::Fusion { prefetch, .. }
            | QueryExpr::Rerank { prefetch, .. }
            | QueryExpr::CrossRerank { prefetch, .. } => prefetch.len(),
            QueryExpr::Hybrid { .. } => 2,
            _ => 0,
        };
        if prefetch_count > 0 && weights.len() != prefetch_count {
            return Err(QqlError::validation(
                "QQL-PLAN-RRF-WEIGHTS",
                format!(
                    "rrf_weights contains {} values but fusion has {} prefetches",
                    weights.len(),
                    prefetch_count
                ),
                None,
            ));
        }
    }
    Ok(())
}

fn validate_query_expr(expression: &QueryExpr) -> Result<(), QqlError> {
    validate_query_target_kinds(expression)?;
    validate_recommend_average_dims(expression)?;
    let prefetch = match expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => prefetch,
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => return Ok(()),
    };

    match expression {
        QueryExpr::Fusion { .. } if prefetch.is_empty() => {
            return Err(QqlError::validation(
                "QQL-PLAN-FUSION-PREFETCH",
                "FUSION requires at least one prefetch",
                None,
            ));
        }
        QueryExpr::Rerank { using: None, .. } => {
            return Err(QqlError::validation(
                "QQL-PLAN-RERANK-USING",
                "RERANK requires a non-empty USING vector name",
                None,
            ));
        }
        QueryExpr::Rerank { .. } if prefetch.is_empty() => {
            return Err(QqlError::validation(
                "QQL-PLAN-RERANK-PREFETCH",
                "RERANK requires at least one prefetch",
                None,
            ));
        }
        _ => {}
    }

    for item in prefetch {
        if let qql_core::ast::PrefetchSource::Query(query) = &item.source {
            validate_query_stmt(query)?;
        }
    }
    Ok(())
}

fn validate_query_target_kinds(expression: &QueryExpr) -> Result<(), QqlError> {
    let (target, inputs): (Option<&VectorTarget>, Vec<&QueryInput>) = match expression {
        QueryExpr::Nearest { input, using, .. } => (using.as_ref(), vec![input]),
        QueryExpr::Recommend {
            positive,
            negative,
            using,
            ..
        } => (
            using.as_ref(),
            positive.iter().chain(negative.iter()).collect(),
        ),
        QueryExpr::Context { pairs, using, .. } => (
            using.as_ref(),
            pairs
                .iter()
                .flat_map(|pair| [&pair.positive, &pair.negative])
                .collect(),
        ),
        QueryExpr::Discover {
            target,
            context,
            using,
            ..
        } => {
            let mut inputs = vec![target];
            inputs.extend(
                context
                    .iter()
                    .flat_map(|pair| [&pair.positive, &pair.negative]),
            );
            (using.as_ref(), inputs)
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            using,
            ..
        } => {
            let mut inputs = vec![target];
            inputs.extend(feedback.iter().map(|item| &item.example));
            (using.as_ref(), inputs)
        }
        QueryExpr::Rerank { input, using, .. } => {
            if using
                .as_ref()
                .and_then(|target| target.kind)
                .is_some_and(|kind| kind != VectorKind::Dense)
            {
                return Err(query_kind_error("RERANK requires a dense vector target"));
            }
            (using.as_ref(), vec![input])
        }
        _ => return Ok(()),
    };

    let Some(target_kind) = target.and_then(|target| target.kind) else {
        return Ok(());
    };
    for input in inputs {
        let input_kind = match input {
            QueryInput::Vector(VectorValue::Dense(_) | VectorValue::MultiDense(_)) => {
                Some(VectorKind::Dense)
            }
            QueryInput::Vector(VectorValue::Sparse { .. }) => Some(VectorKind::Sparse),
            QueryInput::Text { .. } | QueryInput::Image { .. } | QueryInput::Point(_) => None,
        };
        if input_kind.is_some_and(|kind| kind != target_kind) {
            return Err(query_kind_error(
                "query input vector type does not match the USING vector kind",
            ));
        }
    }
    Ok(())
}

/// The recommend `average_vector` strategy (also the server default) folds all
/// positive/negative examples into a single query vector, so every inline
/// example vector must share the same shape. Upstream Qdrant rejects
/// mismatched dimensions (#10374); we fail fast at plan time. Point-ID / TEXT
/// examples have no known shape here and are skipped (resolved later).
fn validate_recommend_average_dims(expression: &QueryExpr) -> Result<(), QqlError> {
    let QueryExpr::Recommend {
        positive,
        negative,
        strategy,
        ..
    } = expression
    else {
        return Ok(());
    };
    // Only average_vector requires a shared shape; best_score / sum_scores
    // score each example independently.
    if matches!(
        strategy,
        Some(qql_core::ast::RecommendStrategy::BestScore)
            | Some(qql_core::ast::RecommendStrategy::SumScores)
    ) {
        return Ok(());
    }

    // Dense → (1, dim); MultiDense → (rows, row dim). Ragged rows have no
    // single dimension, so their shape is unknown here — skip them and let the
    // backend validate. Sparse examples have no average semantics and are
    // skipped as well.
    let shape_of = |value: &VectorValue| -> Option<(usize, usize)> {
        match value {
            VectorValue::Dense(dims) => Some((1, dims.len())),
            VectorValue::MultiDense(rows) => {
                let dim = rows.first().map_or(0, Vec::len);
                rows.iter()
                    .all(|row| row.len() == dim)
                    .then_some((rows.len(), dim))
            }
            VectorValue::Sparse { .. } => None,
        }
    };

    let mut expected: Option<(usize, usize)> = None;
    for input in positive.iter().chain(negative.iter()) {
        let QueryInput::Vector(value) = input else {
            continue;
        };
        let Some(shape) = shape_of(value) else {
            continue;
        };
        match expected {
            None => expected = Some(shape),
            Some(prev) if prev == shape => {}
            Some(prev) => {
                return Err(QqlError::validation(
                    "QQL-PLAN-RECOMMEND-AVERAGE",
                    alloc::format!(
                        "average_vector examples must share one dimension: found ({}, {}) and ({}, {}) rows x dims",
                        prev.0, prev.1, shape.0, shape.1
                    ),
                    None,
                ));
            }
        }
    }
    Ok(())
}

fn query_kind_error(message: &'static str) -> QqlError {
    QqlError::validation("QQL-PLAN-VECTOR-KIND", message, None)
}

fn plan_cross_rerank(
    outer: &qql_core::ast::QueryStmt,
    collection: &str,
    query_text: &str,
    model: &str,
    field: &Option<String>,
    prefetch: &[qql_core::ast::Prefetch],
) -> Result<PlannedOperation, QqlError> {
    use qql_core::ast::{PrefetchSource, QueryCollection};

    if prefetch.is_empty() {
        return Err(QqlError::validation(
            "QQL-PLAN-CROSS-RERANK-PREFETCH",
            "CROSS RERANK requires at least one PREFETCH",
            None,
        ));
    }
    if query_text.is_empty() {
        return Err(QqlError::validation(
            "QQL-PLAN-CROSS-RERANK-QUERY",
            "CROSS RERANK query text must not be empty",
            None,
        ));
    }
    if model.is_empty() {
        return Err(QqlError::validation(
            "QQL-PLAN-CROSS-RERANK-MODEL",
            "CROSS RERANK MODEL must not be empty",
            None,
        ));
    }
    let field = field
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("text")
        .to_string();

    let mut candidates = Vec::with_capacity(prefetch.len());
    for pref in prefetch {
        let mut sub = match &pref.source {
            PrefetchSource::Cte(name) => {
                let cte = outer
                    .ctes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(name));
                let Some(cte) = cte else {
                    return Err(QqlError::validation(
                        "QQL-PLAN-CROSS-RERANK-CTE",
                        format!("PREFETCH references unknown CTE '{name}'"),
                        None,
                    ));
                };
                (*cte.query).clone()
            }
            PrefetchSource::Query(q) => (**q).clone(),
        };
        if matches!(sub.collection, QueryCollection::Inherited) {
            sub.collection = QueryCollection::Explicit(collection.to_string());
        }
        // Candidate stage needs document text for pair scoring.
        ensure_payload_field(&mut sub, &field);
        if let Some(f) = &pref.filter {
            sub.filter = Some(f.clone());
        }
        let planned = plan(&Stmt::Query(Box::new(sub)))?;
        match planned {
            PlannedOperation::Query {
                collection: c,
                request,
            } => candidates.push((c, request)),
            other => {
                return Err(QqlError::validation(
                    "QQL-PLAN-CROSS-RERANK-CANDIDATE",
                    format!(
                        "CROSS RERANK prefetch must plan as a search query, got {}",
                        other.operation_label()
                    ),
                    None,
                ));
            }
        }
    }

    Ok(PlannedOperation::CrossRerank {
        collection: collection.to_string(),
        query: query_text.to_string(),
        model: model.to_string(),
        field,
        limit: outer.page.limit.unwrap_or(10),
        offset: outer.page.offset.unwrap_or(0),
        candidates,
    })
}

fn ensure_payload_field(query: &mut qql_core::ast::QueryStmt, field: &str) {
    use qql_core::ast::{PayloadSelector, QueryOutput};
    match &mut query.output {
        QueryOutput {
            payload: None | Some(PayloadSelector::None),
            ..
        } => {
            query.output.payload = Some(PayloadSelector::Include(vec![field.to_string()]));
        }
        QueryOutput {
            payload: Some(PayloadSelector::Include(fields)),
            ..
        } => {
            if !fields.iter().any(|f| f.eq_ignore_ascii_case(field)) {
                fields.push(field.to_string());
            }
        }
        QueryOutput {
            payload: Some(PayloadSelector::All | PayloadSelector::Exclude(_)),
            ..
        } => {}
    }
}

/// Why a planned operation cannot become a single Qdrant REST route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestProjectionError {
    /// Client-side only (e.g. CROSS RERANK). Compile still exposes `stmt_type`.
    ClientSideOnly {
        /// Statement type name used in error messages.
        stmt_type: &'static str,
    },
}

/// Serialize a plan struct to JSON for the REST body.
///
/// Every plan IR request type is JSON-serializable by construction, so this is
/// only reachable if a `Serialize` impl regresses. A failure must surface as a
/// loud invariant violation — a JSON `null` body would be rejected by the
/// backend with an opaque error. `pub(crate)` so the plan/query/ddl REST
/// projections share the same no-swallow invariant; kept fallible so the
/// property is unit-testable.
pub(crate) fn serialize_body<T: serde::Serialize>(
    req: &T,
) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(req)
}

/// REST projection of a planned operation (HTTP method/path/query/body).
///
/// Client-side operations such as [`PlannedOperation::CrossRerank`] return
/// [`RestProjectionError::ClientSideOnly`] — they must not invent a Qdrant path.
/// Returns `RestProjectionError::ClientSideOnly` for operations that have no
/// single Qdrant REST endpoint (e.g. CROSS RERANK).
pub fn to_rest_route(op: &PlannedOperation) -> Result<Route, RestProjectionError> {
    /// Serialize a plan struct to JSON for the REST body, treating the
    /// (provably unreachable) serialization failure as an invariant violation.
    fn body<T: serde::Serialize>(req: &T) -> Option<serde_json::Value> {
        Some(serialize_body(req).expect("plan IR REST request body serialization failed"))
    }

    /// Read-op query params: timeout, consistency.
    fn read_query(
        timeout: Option<u64>,
        consistency: Option<&crate::types::ReadConsistencyParam>,
    ) -> Vec<(String, String)> {
        let mut q = Vec::new();
        crate::query::push_read_opts(&mut q, timeout, consistency);
        q
    }

    /// Mutation query params: wait + optional shard_key.
    fn mut_query(shard_key: Option<&str>) -> Vec<(String, String)> {
        let mut q = vec![("wait".into(), "true".into())];
        if let Some(sk) = shard_key {
            q.push(("shard_key".into(), sk.to_owned()));
        }
        q
    }

    Ok(match op {
        PlannedOperation::Query {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query"),
            query: read_query(request.timeout, request.consistency.as_ref()),
            body: body(request),
        },
        PlannedOperation::QueryGroups {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query/groups"),
            query: read_query(request.timeout, request.consistency.as_ref()),
            body: body(request),
        },
        PlannedOperation::GetPoints {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points"),
            query: Vec::new(),
            body: body(request),
        },
        PlannedOperation::Scroll {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/scroll"),
            query: Vec::new(),
            body: body(request),
        },
        PlannedOperation::Count {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/count"),
            query: Vec::new(),
            body: body(request),
        },
        PlannedOperation::Facet {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/facet"),
            query: Vec::new(),
            body: body(request),
        },
        PlannedOperation::Upsert {
            collection,
            request,
            wait,
        } => {
            let mut query = Vec::new();
            if *wait {
                query.push(("wait".into(), "true".into()));
            }
            if let Some(ref sk) = request.shard_key {
                query.push(("shard_key".into(), sk.clone()));
            }
            Route {
                method: Method::Put,
                path: format!("/collections/{collection}/points"),
                query,
                body: body(request),
            }
        }
        PlannedOperation::Delete {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/delete"),
            query: mut_query(request.shard_key.as_deref()),
            body: body(request),
        },
        PlannedOperation::ClearPayload {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload/clear"),
            query: mut_query(request.shard_key.as_deref()),
            body: body(request),
        },
        PlannedOperation::DeletePayload {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload/delete"),
            query: mut_query(request.shard_key.as_deref()),
            body: body(request),
        },
        PlannedOperation::DeleteVectors {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/vectors/delete"),
            query: mut_query(request.shard_key.as_deref()),
            body: body(request),
        },
        PlannedOperation::UpdateVectors {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/points/vectors"),
            query: mut_query(request.shard_key.as_deref()),
            body: body(request),
        },
        PlannedOperation::UpdatePayload {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload"),
            query: mut_query(request.shard_key.as_deref()),
            body: body(request),
        },
        // DDL: REST shapes differ from plan IR — use OpenAPI projection fns
        PlannedOperation::CreateCollection {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: Some(crate::ddl::create_collection_rest_body(request)),
        },
        PlannedOperation::UpdateCollection {
            collection,
            request,
        } => Route {
            method: Method::Patch,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: Some(crate::ddl::update_collection_rest_body(request)),
        },
        PlannedOperation::CreateIndex {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/index"),
            query: Vec::new(),
            body: Some(crate::ddl::create_index_rest_body(request)),
        },
        PlannedOperation::CreateShardKey {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/shards"),
            query: Vec::new(),
            body: body(request),
        },
        PlannedOperation::DropShardKey {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/shards/delete"),
            query: Vec::new(),
            body: body(request),
        },
        // Bodyless
        PlannedOperation::DropCollection { collection } => Route {
            method: Method::Delete,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::DropIndex { collection, field } => Route {
            method: Method::Delete,
            path: format!("/collections/{collection}/index/{field}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::ListCollections => Route {
            method: Method::Get,
            path: "/collections".into(),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::GetCollection { collection } => Route {
            method: Method::Get,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::ListShardKeys { collection } => Route {
            method: Method::Get,
            path: format!("/collections/{collection}/shards"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::GetQuotas => Route {
            method: Method::Get,
            path: "/quotas".into(),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::SetQuotas { request } => {
            let mut query = Vec::new();
            if let Some(wait) = request.wait {
                query.push(("wait".into(), wait.to_string()));
            }
            Route {
                method: Method::Put,
                path: "/quotas".into(),
                query,
                body: body(request),
            }
        }
        PlannedOperation::CrossRerank { .. } => {
            return Err(RestProjectionError::ClientSideOnly {
                stmt_type: "cross_rerank",
            });
        }
    })
}
/// Plan a statement and project it to a REST route in one call.
///
/// Returns `QQL-REST-CLIENT-SIDE` for client-side operations (e.g. CROSS
/// RERANK) that have no single Qdrant REST endpoint.
pub fn try_route(statement: &Stmt) -> Result<Route, QqlError> {
    let op = plan(statement)?;
    to_rest_route(&op).map_err(|err| match err {
        RestProjectionError::ClientSideOnly { stmt_type } => QqlError::validation(
            "QQL-REST-CLIENT-SIDE",
            format!(
                "{stmt_type} is client-side and has no single Qdrant REST route; \
                 execute via the runtime CROSS RERANK path"
            ),
            None,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::{PageSpec, QueryInput, QueryOutput, QueryStmt};
    use qql_core::parser::Parser;

    /// Build a minimal recommend statement with inline example vectors.
    fn recommend_stmt(
        strategy: Option<qql_core::ast::RecommendStrategy>,
        positive: Vec<QueryInput>,
        negative: Vec<QueryInput>,
    ) -> Stmt {
        use qql_core::ast::QueryCollection;
        Stmt::Query(Box::new(QueryStmt {
            ctes: Vec::new(),
            collection: QueryCollection::Explicit("docs".into()),
            expression: QueryExpr::Recommend {
                positive,
                negative,
                strategy,
                using: None,
                prefetch: Vec::new(),
            },
            filter: None,
            params: None,
            score_threshold: None,
            group: None,
            output: QueryOutput::default(),
            page: PageSpec {
                limit: Some(5),
                offset: None,
            },
            shard_key: None,
        }))
    }

    #[test]
    fn recommend_average_rejects_mismatched_example_dims() {
        // Qdrant #10374: the average API folds examples into one vector, so
        // mismatched dimensions are rejected (client-side at plan time here).
        let stmt = recommend_stmt(
            None,
            vec![QueryInput::Vector(VectorValue::Dense(vec![0.1, 0.2]))],
            vec![QueryInput::Vector(VectorValue::Dense(vec![0.3, 0.4, 0.5]))],
        );
        let err = plan(&stmt).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-RECOMMEND-AVERAGE");
    }

    #[test]
    fn recommend_average_accepts_matching_example_dims() {
        let stmt = recommend_stmt(
            Some(qql_core::ast::RecommendStrategy::AverageVector),
            vec![
                QueryInput::Vector(VectorValue::Dense(vec![0.1, 0.2])),
                QueryInput::Vector(VectorValue::Dense(vec![0.3, 0.4])),
            ],
            vec![QueryInput::Vector(VectorValue::Dense(vec![0.5, 0.6]))],
        );
        assert!(plan(&stmt).is_ok());
    }

    #[test]
    fn recommend_best_score_allows_mismatched_example_dims() {
        // best_score scores each example independently — no shared shape needed.
        let stmt = recommend_stmt(
            Some(qql_core::ast::RecommendStrategy::BestScore),
            vec![QueryInput::Vector(VectorValue::Dense(vec![0.1, 0.2]))],
            vec![QueryInput::Vector(VectorValue::Dense(vec![0.3, 0.4, 0.5]))],
        );
        assert!(plan(&stmt).is_ok());
    }

    #[test]
    fn recommend_average_rejects_mismatched_multidense_row_counts() {
        let stmt = recommend_stmt(
            None,
            vec![QueryInput::Vector(VectorValue::MultiDense(vec![
                vec![0.1, 0.2],
                vec![0.3, 0.4],
            ]))],
            vec![QueryInput::Vector(VectorValue::MultiDense(vec![vec![
                0.5, 0.6,
            ]]))],
        );
        let err = plan(&stmt).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-RECOMMEND-AVERAGE");
    }

    #[test]
    fn recommend_average_defers_ragged_multidense_to_the_backend() {
        // Ragged rows have no single dimension — the plan cannot judge shape,
        // so it must defer instead of comparing first-row lengths.
        let stmt = recommend_stmt(
            None,
            vec![QueryInput::Vector(VectorValue::MultiDense(vec![
                vec![0.1, 0.2, 0.3],
                vec![0.4, 0.5],
            ]))],
            vec![QueryInput::Vector(VectorValue::MultiDense(vec![
                vec![0.6, 0.7],
                vec![0.8, 0.9, 1.0],
            ]))],
        );
        assert!(plan(&stmt).is_ok());
    }

    #[test]
    fn plan_rejects_inherited_top_level() {
        // Parser already rejects this, but programmatic AST must fail at plan.
        use qql_core::ast::*;
        let stmt = Stmt::Query(Box::new(QueryStmt {
            ctes: vec![],
            collection: QueryCollection::Inherited,
            expression: QueryExpr::SampleRandom,
            filter: None,
            params: None,
            score_threshold: None,
            group: None,
            output: QueryOutput::default(),
            page: PageSpec {
                limit: Some(5),
                offset: None,
            },
            shard_key: None,
        }));
        let err = plan(&stmt).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    }

    #[test]
    fn plan_and_route_agree_on_query() {
        let stmt = Parser::parse("QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 5;").unwrap();
        let op = plan(&stmt).unwrap();
        let route = to_rest_route(&op).expect("rest route");
        assert_eq!(route.path, "/collections/docs/points/query");
        assert!(route.body.is_some());
    }

    #[test]
    fn create_and_alter_are_distinct() {
        let create = Parser::parse("CREATE COLLECTION docs (dense VECTOR(4, COSINE));").unwrap();
        let alter =
            Parser::parse("ALTER COLLECTION docs WITH PARAMS (replication_factor = 2);").unwrap();
        assert!(matches!(
            plan(&create).unwrap(),
            PlannedOperation::CreateCollection { .. }
        ));
        assert!(matches!(
            plan(&alter).unwrap(),
            PlannedOperation::UpdateCollection { .. }
        ));
        let alter_route = try_route(&alter).unwrap();
        assert_eq!(alter_route.method, Method::Patch);
        assert!(alter_route.body.is_some());
    }

    #[test]
    fn plan_rejects_malformed_rerank() {
        use qql_core::ast::{
            PageSpec, QueryInput, QueryOutput, QueryStmt, VectorKind, VectorTarget,
        };
        let stmt_empty_using = Stmt::Query(Box::new(QueryStmt {
            ctes: Vec::new(),
            collection: QueryCollection::Explicit("docs".into()),
            expression: QueryExpr::Rerank {
                input: QueryInput::Text {
                    text: "rerank text".into(),
                    model: None,
                },
                model: "colbert-v2".into(),
                using: None,
                prefetch: vec![qql_core::ast::Prefetch {
                    source: qql_core::ast::PrefetchSource::Query(Box::new(QueryStmt {
                        ctes: Vec::new(),
                        collection: QueryCollection::Inherited,
                        expression: QueryExpr::SampleRandom,
                        filter: None,
                        params: None,
                        score_threshold: None,
                        group: None,
                        output: QueryOutput::default(),
                        page: PageSpec {
                            limit: Some(10),
                            offset: None,
                        },
                        shard_key: None,
                    })),
                    filter: None,
                    score_threshold: None,
                    lookup: None,
                }],
            },
            filter: None,
            params: None,
            score_threshold: None,
            group: None,
            output: QueryOutput::default(),
            page: PageSpec {
                limit: Some(5),
                offset: None,
            },
            shard_key: None,
        }));
        assert_eq!(
            plan(&stmt_empty_using).unwrap_err().kind,
            qql_core::error::ErrorKind::Validation
        );

        let stmt_empty_prefetch = Stmt::Query(Box::new(QueryStmt {
            ctes: Vec::new(),
            collection: QueryCollection::Explicit("docs".into()),
            expression: QueryExpr::Rerank {
                input: QueryInput::Text {
                    text: "rerank text".into(),
                    model: None,
                },
                model: "colbert-v2".into(),
                using: Some(VectorTarget {
                    name: "dense".into(),
                    kind: Some(VectorKind::Dense),
                    multi: false,
                }),
                prefetch: Vec::new(),
            },
            filter: None,
            params: None,
            score_threshold: None,
            group: None,
            output: QueryOutput::default(),
            page: PageSpec {
                limit: Some(5),
                offset: None,
            },
            shard_key: None,
        }));
        assert_eq!(
            plan(&stmt_empty_prefetch).unwrap_err().kind,
            qql_core::error::ErrorKind::Validation
        );
    }

    #[test]
    fn delete_payload_planning_and_routing() {
        let stmt = qql_core::parser::Parser::parse(
            "DELETE PAYLOAD draft, temp_token FROM docs WHERE status = 'archived' SHARD 'tenant_1';",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();

        assert_eq!(op.operation_label(), "DELETE_PAYLOAD");
        assert_eq!(op.compile_stmt_type(), "delete_payload");
        assert_eq!(op.collection(), Some("docs"));
        assert_eq!(op.shard_key(), Some("tenant_1"));

        if let PlannedOperation::DeletePayload {
            collection,
            request,
        } = &op
        {
            assert_eq!(collection, "docs");
            assert_eq!(request.keys, vec!["draft", "temp_token"]);
            assert_eq!(request.shard_key.as_deref(), Some("tenant_1"));
            assert!(request.filter.is_some());
        } else {
            panic!("expected DeletePayload operation");
        }

        let route = crate::to_rest_route(&op).unwrap();
        assert_eq!(route.method, crate::Method::Post);
        assert_eq!(route.path, "/collections/docs/points/payload/delete");
    }

    #[test]
    fn count_exact_planning() {
        // Grammar `count` order: WHERE → SHARD → WITH (grammar.pest).
        let stmt = qql_core::parser::Parser::parse(
            "COUNT FROM docs WHERE active = true SHARD 'tenant_2' WITH (exact = true);",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();

        assert_eq!(op.operation_label(), "COUNT");
        assert_eq!(op.collection(), Some("docs"));
        assert_eq!(op.shard_key(), Some("tenant_2"));

        if let PlannedOperation::Count { request, .. } = op {
            assert_eq!(request.exact, Some(true));
            assert_eq!(request.shard_key.as_deref(), Some("tenant_2"));
            assert!(request.filter.is_some());
        } else {
            panic!("expected Count operation");
        }
    }

    #[test]
    fn facet_planning_and_routing() {
        let stmt = qql_core::parser::Parser::parse(
            "FACET room_type FROM stays WHERE price < 100 LIMIT 10 EXACT true;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();

        assert_eq!(op.operation_label(), "FACET");
        assert_eq!(op.collection(), Some("stays"));

        if let PlannedOperation::Facet { request, .. } = &op {
            assert_eq!(request.key, "room_type");
            assert_eq!(request.limit, Some(10));
            assert_eq!(request.exact, Some(true));
            assert!(request.filter.is_some());
        } else {
            panic!("expected Facet operation");
        }

        let route = to_rest_route(&op).unwrap();
        assert_eq!(route.path, "/collections/stays/facet");
        assert_eq!(route.method, Method::Post);
        let body = route.body.unwrap();
        assert_eq!(body["key"], "room_type");
        assert_eq!(body["limit"], 10);
        assert_eq!(body["exact"], true);
    }

    #[test]
    fn plan_rejects_inherited_collection_for_count() {
        use qql_core::ast::*;
        let stmt = Stmt::Count(Box::new(CountStmt {
            collection: QueryCollection::Inherited,
            filter: None,
            shard_key: None,
            exact: None,
        }));
        let err = plan(&stmt).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-COLLECTION");
    }

    #[test]
    fn body_serialization_failure_is_an_error_not_null() {
        struct FailingSerialize;
        impl serde::Serialize for FailingSerialize {
            fn serialize<S: serde::Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("injected serialization failure"))
            }
        }
        assert!(
            serialize_body(&FailingSerialize).is_err(),
            "a serialization failure must surface as Err, never a JSON null body"
        );

        // Sanity: real plan bodies still serialize to non-null JSON.
        let op =
            plan(&Parser::parse("QUERY TEXT 'x' MODEL 'e5' FROM docs LIMIT 5;").unwrap()).unwrap();
        let route = to_rest_route(&op).expect("rest route");
        assert!(route.body.is_some());
        assert_ne!(route.body, Some(serde_json::Value::Null));
    }
}
