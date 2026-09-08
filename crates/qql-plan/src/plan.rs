//! Canonical fallible planner: AST → [`PlannedOperation`].
//!
//! `PlannedOperation` is the transport-neutral source of truth. REST routes
//! are a projection (`to_rest_route`). gRPC converts the same typed operation.

use crate::ddl::{
    lower_alter_collection, lower_create_collection, lower_create_index, lower_set_quota,
};
use crate::mutation::{
    lower_clear_payload_request, lower_delete_payload_request, lower_delete_request,
    lower_delete_vector_request, lower_scroll_request, lower_update_payload_request,
    lower_update_vector_request, lower_upsert_request,
};
use crate::query::{lower_query_groups_request, lower_query_request};
use crate::rerank::plan_cross_rerank;
use crate::types::*;
use crate::validate::validate_query_stmt;
use qql_core::ast::{QueryCollection, QueryExpr, Stmt};
use qql_core::error::QqlError;

pub use crate::routing::{RestProjectionError, to_rest_route, try_route};

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
    /// In-database facet aggregation (REST `POST /collections/{collection}/facet` or gRPC `Points.Facet`).
    Facet {
        /// Target collection name.
        collection: String,
        /// Lowered facet request body.
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
        /// Wait for deletion.
        wait: bool,
    },
    /// Merge payload keys: `POST /collections/{c}/points/payload`.
    UpdatePayload {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/payload` request body.
        request: UpdatePayloadRequest,
        /// Wait for update.
        wait: bool,
    },
    /// Drop all payload: `POST /collections/{c}/points/payload/clear`.
    ClearPayload {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/payload/clear` request body.
        request: ClearPayloadRequest,
        /// Wait for clear.
        wait: bool,
    },
    /// Remove payload keys: `POST /collections/{c}/points/payload/delete`.
    DeletePayload {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/payload/delete` request body.
        request: DeletePayloadRequest,
        /// Wait for delete.
        wait: bool,
    },
    /// Replace point vectors: `PUT /collections/{c}/points/vectors`.
    UpdateVectors {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/vectors` request body.
        request: UpdateVectorRequest,
        /// Wait for update.
        wait: bool,
    },
    /// Remove named vectors: `POST /collections/{c}/points/vectors/delete`.
    DeleteVectors {
        /// Target collection name.
        collection: String,
        /// Lowered `/points/vectors/delete` request body.
        request: DeleteVectorRequest,
        /// Wait for delete.
        wait: bool,
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
        /// Wait for index creation.
        wait: bool,
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

    /// Shard key carried on the plan, when present, with its keyword /
    /// numeric form preserved.
    pub fn shard_key(&self) -> Option<&crate::semantic::PlanShardKey> {
        match self {
            PlannedOperation::Query { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::QueryGroups { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::GetPoints { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::Scroll { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::Count { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::Facet { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::Upsert { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::Delete { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::UpdatePayload { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::ClearPayload { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::DeletePayload { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::UpdateVectors { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::DeleteVectors { request, .. } => request.shard_key.as_ref(),
            PlannedOperation::CreateShardKey { request, .. } => Some(&request.shard_key),
            PlannedOperation::DropShardKey { request, .. } => Some(&request.shard_key),
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
pub use crate::batch::{
    BatchKey, batch_item_error, build_query_batch, build_update_batch, statement_batch_key,
    verify_batch_cardinality,
};

/// An unbound parameter placeholder (`:name` / `?idx`) that reaches planning
/// would ship a broken request — the string path with no `params` used to
/// send the raw placeholder to Qdrant and get a 422 back. Probe with the
/// binder's own traversal (no-op lookup): any surviving placeholder raises
/// `QQL-BIND-MISSING-PARAM`, the same error `Stmt.bind` raises earlier on
/// the prepared path.
///
/// Public so the executor can probe *before* schema resolution (network);
/// [`plan`] probes again as the compile-path gate.
pub fn ensure_no_unbound_params(statement: &Stmt) -> Result<(), QqlError> {
    qql_core::params::validate_no_unbound_params(statement)
}

/// Fallible planner — the single source of truth for statement → operation.
pub fn plan(statement: &Stmt) -> Result<PlannedOperation, QqlError> {
    ensure_no_unbound_params(statement)?;
    lower_statement_to_planned(statement)
}

/// Fallible template planner for prepared statements.
///
/// Permits unbound vector parameters (`:name` or `?N`) so statements can be
/// pre-planned into a [`PlannedOperation`], while rejecting unbound scalar parameters
/// (e.g. filters, pagination, point IDs) which cannot be bound at the IR layer.
pub fn plan_template(statement: &Stmt) -> Result<PlannedOperation, QqlError> {
    qql_core::params::validate_no_unbound_scalar_params(statement)?;
    lower_statement_to_planned(statement)
}

pub(crate) fn lower_statement_to_planned(statement: &Stmt) -> Result<PlannedOperation, QqlError> {
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
                        shard_key: query
                            .shard_key
                            .as_ref()
                            .map(crate::semantic::PlanShardKey::from),
                    },
                });
            }

            if let QueryExpr::CrossRerank {
                query: qtext,
                model,
                field,
                prefetch,
                ..
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
            wait: upsert
                .wait
                .unwrap_or(upsert.embedding.is_some() || !upsert.embed.is_empty()),
        }),
        Stmt::Delete(delete) => Ok(PlannedOperation::Delete {
            collection: delete.collection.clone(),
            request: lower_delete_request(delete),
            wait: delete.wait.unwrap_or(true),
        }),
        Stmt::ClearPayload(clear) => Ok(PlannedOperation::ClearPayload {
            collection: clear.collection.clone(),
            request: lower_clear_payload_request(clear),
            wait: clear.wait.unwrap_or(true),
        }),
        Stmt::DeletePayload(del) => Ok(PlannedOperation::DeletePayload {
            collection: del.collection.clone(),
            request: lower_delete_payload_request(del),
            wait: del.wait.unwrap_or(true),
        }),
        Stmt::DeleteVector(del_vec) => Ok(PlannedOperation::DeleteVectors {
            collection: del_vec.collection.clone(),
            request: lower_delete_vector_request(del_vec),
            wait: del_vec.wait.unwrap_or(true),
        }),
        Stmt::UpdateVector(update) => Ok(PlannedOperation::UpdateVectors {
            collection: update.collection.clone(),
            request: lower_update_vector_request(update),
            wait: update.wait.unwrap_or(true),
        }),
        Stmt::UpdatePayload(update) => Ok(PlannedOperation::UpdatePayload {
            collection: update.collection.clone(),
            request: lower_update_payload_request(update),
            wait: update.wait.unwrap_or(true),
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
            wait: index.wait.unwrap_or(true),
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
                    shard_key: count
                        .shard_key
                        .as_ref()
                        .map(crate::semantic::PlanShardKey::from),
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
            Ok(PlannedOperation::Facet {
                collection,
                request: FacetRequest {
                    key: facet.key.clone(),
                    limit: facet.limit,
                    filter,
                    exact: facet.exact,
                    shard_key: facet
                        .shard_key
                        .as_ref()
                        .map(crate::semantic::PlanShardKey::from),
                },
            })
        }
        Stmt::CreateShardKey(sk) => Ok(PlannedOperation::CreateShardKey {
            collection: sk.collection.clone(),
            request: CreateShardKeyRequest {
                shard_key: crate::semantic::PlanShardKey::from(&sk.shard_key),
                shards_number: sk.shards_number,
                replication_factor: sk.replication_factor,
            },
        }),
        Stmt::DropShardKey(sk) => Ok(PlannedOperation::DropShardKey {
            collection: sk.collection.clone(),
            request: DropShardKeyRequest {
                shard_key: crate::semantic::PlanShardKey::from(&sk.shard_key),
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

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::{PageSpec, QueryInput, QueryOutput, QueryStmt, VectorValue};
    use qql_core::parser::Parser;

    #[test]
    fn unbound_params_fail_at_plan_time_with_the_binder_code() {
        // N2: the string path with no params used to ship the raw placeholder
        // to Qdrant and get a 422 back. Planning is the execution gate, so an
        // unbound template must raise the binder's own missing-param error.
        for source in [
            "QUERY VECTOR :qvec FROM docs USING dense LIMIT 1;",
            "QUERY :v FROM docs USING dense LIMIT :lim;",
            "QUERY TEXT :q FROM docs LIMIT 5;",
        ] {
            let stmt = Parser::parse(source).expect("template must parse");
            let err = plan(&stmt).expect_err("unbound template must not plan");
            assert_eq!(err.code, "QQL-BIND-MISSING-PARAM", "{err:?}");
            assert!(err.message.contains("missing value"), "{err:?}");
        }
    }

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
                ..Default::default()
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
                ..Default::default()
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
                    text_param: None,
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
                            ..Default::default()
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
                ..Default::default()
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
                    text_param: None,
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
                ..Default::default()
            },
            shard_key: None,
        }));
        assert_eq!(
            plan(&stmt_empty_prefetch).unwrap_err().kind,
            qql_core::error::ErrorKind::Validation
        );
    }

    #[test]
    fn delete_payload_batches_with_mutations() {
        // P1: DeletePayload is Mutation-batchable and has an UpdateOperation
        // form, so contiguous same-collection runs build one update batch
        // (REST DeletePayloadOperation / gRPC delete_payload = 5).
        use crate::batch::statement_batch_key;
        use crate::mutation::planned_to_update_operation;
        let s1 =
            qql_core::parser::Parser::parse("DELETE PAYLOAD a FROM docs WHERE id = 1;").unwrap();
        let s2 =
            qql_core::parser::Parser::parse("DELETE PAYLOAD b FROM docs WHERE id = 2;").unwrap();
        assert!(statement_batch_key(&s1).is_some());
        let op1 = plan(&s1).unwrap();
        let op2 = plan(&s2).unwrap();
        assert_eq!(op1.batch_key(), op2.batch_key());
        assert!(planned_to_update_operation(&op1).is_some());
        let (collection, labels, batch) = crate::batch::build_update_batch(&[op1, op2]).unwrap();
        assert_eq!(collection, "docs");
        assert_eq!(labels, vec!["DELETE_PAYLOAD", "DELETE_PAYLOAD"]);
        assert_eq!(batch.operations.len(), 2);
        let json = serde_json::to_value(&batch).unwrap();
        assert!(json["operations"][0].get("delete_payload").is_some());
        assert_eq!(
            json["operations"][0]["delete_payload"]["keys"],
            serde_json::json!(["a"])
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
        assert_eq!(
            op.shard_key(),
            Some(&crate::semantic::PlanShardKey::Keyword("tenant_1".into()))
        );

        if let PlannedOperation::DeletePayload {
            collection,
            request,
            ..
        } = &op
        {
            assert_eq!(collection, "docs");
            assert_eq!(request.keys, vec!["draft", "temp_token"]);
            assert_eq!(
                request.shard_key,
                Some(crate::semantic::PlanShardKey::Keyword("tenant_1".into()))
            );
            assert!(request.filter.is_some());
        } else {
            panic!("expected DeletePayload operation");
        }

        let route = crate::to_rest_route(&op).unwrap();
        assert_eq!(route.method, crate::Method::Post);
        assert_eq!(route.path, "/collections/docs/points/payload/delete");
    }

    #[test]
    fn modelless_document_serializes_empty_model_placeholder() {
        // P3: model-less plans keep "model": "" for the executor to fill;
        // offline bodies require preparation before dispatch.
        let stmt =
            qql_core::parser::Parser::parse("QUERY TEXT 'hello' FROM docs LIMIT 5;").unwrap();
        let op = plan(&stmt).unwrap();
        let route = to_rest_route(&op).unwrap();
        let body = route.body.unwrap();
        assert_eq!(body["query"]["nearest"]["text"], "hello");
        assert_eq!(body["query"]["nearest"]["model"], "");
    }

    #[test]
    fn param_only_upsert_template_plans_empty_points_for_splice() {
        // P5: whole-point placeholders splice at execution time; the template
        // plan holds inline points only. An all-placeholder template is empty
        // and must take the splice path, never dispatch directly.
        let stmt = qql_core::parser::Parser::parse("UPSERT INTO docs VALUES :p;").unwrap();
        let op = plan_template(&stmt).unwrap();
        let PlannedOperation::Upsert { request, .. } = &op else {
            panic!("expected Upsert, got {op:?}");
        };
        assert!(
            request.is_empty(),
            "param-only template must plan no points"
        );
    }

    #[test]
    fn statement_and_planned_batch_keys_agree_on_queries() {
        // P10: statement-level and planned keys must agree; CrossRerank is
        // client-side Single on both levels, Points is never batched.
        let cases = [
            ("QUERY TEXT 'x' FROM docs LIMIT 1;", true),
            ("QUERY POINTS (1) FROM docs;", false),
            (
                "QUERY CROSS RERANK TEXT 'q' MODEL 'm' ON FIELD body FROM docs PREFETCH (QUERY TEXT 'q' FROM docs USING dense LIMIT 5) LIMIT 10;",
                false,
            ),
        ];
        for (source, expect_batchable) in cases {
            let stmt = qql_core::parser::Parser::parse(source).unwrap();
            let ast_key = crate::batch::statement_batch_key(&stmt);
            let planned = plan(&stmt).unwrap();
            let plan_key = planned.batch_key();
            assert_eq!(
                ast_key.is_some(),
                expect_batchable,
                "statement key for {source}"
            );
            assert_eq!(
                plan_key.is_some(),
                expect_batchable,
                "planned key for {source}"
            );
        }
    }

    #[test]
    fn group_query_without_group_is_an_error_not_a_panic() {
        // P4: the groups lowerer is pub; a group-less call must error.
        let stmt = qql_core::parser::Parser::parse("QUERY TEXT 'x' FROM docs LIMIT 1;").unwrap();
        let qql_core::ast::Stmt::Query(query) = &stmt else {
            panic!("expected query");
        };
        let err = crate::query::lower_query_groups_request(query).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-GROUP");
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
        assert_eq!(
            op.shard_key(),
            Some(&crate::semantic::PlanShardKey::Keyword("tenant_2".into()))
        );

        if let PlannedOperation::Count { request, .. } = op {
            assert_eq!(request.exact, Some(true));
            assert_eq!(
                request.shard_key,
                Some(crate::semantic::PlanShardKey::Keyword("tenant_2".into()))
            );
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
            crate::routing::serialize_body(&FailingSerialize).is_err(),
            "a serialization failure must surface as Err, never a JSON null body"
        );

        // Sanity: real plan bodies still serialize to non-null JSON.
        let op =
            plan(&Parser::parse("QUERY TEXT 'x' MODEL 'e5' FROM docs LIMIT 5;").unwrap()).unwrap();
        let route = to_rest_route(&op).expect("rest route");
        assert!(route.body.is_some());
        assert_ne!(route.body, Some(serde_json::Value::Null));
    }

    #[test]
    fn cross_rerank_with_cte_prefetch_plans_candidates() {
        let stmt = Parser::parse(
            "WITH candidates AS (QUERY TEXT 'rust query' MODEL 'bge' FROM docs USING dense LIMIT 50) \
             QUERY CROSS RERANK TEXT 'rust query' MODEL 'bge-reranker-large' ON FIELD body \
             FROM docs PREFETCH (candidates) LIMIT 10;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();
        match op {
            PlannedOperation::CrossRerank {
                collection,
                query,
                model,
                field,
                limit,
                offset,
                candidates,
            } => {
                assert_eq!(collection, "docs");
                assert_eq!(query, "rust query");
                assert_eq!(model, "bge-reranker-large");
                assert_eq!(field, "body");
                assert_eq!(limit, 10);
                assert_eq!(offset, 0);
                assert_eq!(candidates.len(), 1);
                assert_eq!(candidates[0].0, "docs");
                assert_eq!(candidates[0].1.limit, Some(50));
            }
            other => panic!("expected CrossRerank, got {other:?}"),
        }
    }
}
