//! Canonical fallible planner: AST → [`PlannedOperation`].
//!
//! `PlannedOperation` is the transport-neutral source of truth. REST routes
//! are a projection (`to_rest_route`). gRPC converts the same typed operation.

use crate::ddl::{lower_alter_collection, lower_create_collection, lower_create_index};
use crate::mutation::{
    lower_clear_payload_request, lower_delete_request, lower_delete_vector_request,
    lower_scroll_request, lower_update_payload_request, lower_update_vector_request,
    lower_upsert_request,
};
use crate::query::{lower_query_groups_request, lower_query_request};
use crate::routing::{RequestBody, Route};
use crate::types::*;
use qql_core::ast::{
    QueryCollection, QueryExpr, QueryInput, Stmt, VectorKind, VectorTarget, VectorValue,
};
use qql_core::error::QqlError;

/// Canonical planned operation. Batch compatibility is determined from this
/// type, not from raw AST.
#[derive(Debug, Clone)]
pub enum PlannedOperation {
    Query {
        collection: String,
        request: QueryRequest,
    },
    QueryGroups {
        collection: String,
        request: QueryGroupsRequest,
    },
    GetPoints {
        collection: String,
        request: PointsRequest,
    },
    Scroll {
        collection: String,
        request: ScrollRequest,
    },
    Count {
        collection: String,
        request: CountRequest,
    },
    Upsert {
        collection: String,
        request: UpsertRequest,
        wait: bool,
    },
    Delete {
        collection: String,
        request: DeleteRequest,
    },
    UpdatePayload {
        collection: String,
        request: UpdatePayloadRequest,
    },
    ClearPayload {
        collection: String,
        request: ClearPayloadRequest,
    },
    UpdateVectors {
        collection: String,
        request: UpdateVectorRequest,
    },
    DeleteVectors {
        collection: String,
        request: DeleteVectorRequest,
    },
    CreateCollection {
        collection: String,
        request: CreateCollectionRequest,
    },
    UpdateCollection {
        collection: String,
        request: UpdateCollectionRequest,
    },
    DropCollection {
        collection: String,
    },
    CreateIndex {
        collection: String,
        request: CreateIndexRequest,
    },
    DropIndex {
        collection: String,
        field: String,
    },
    CreateShardKey {
        collection: String,
        request: CreateShardKeyRequest,
    },
    DropShardKey {
        collection: String,
        request: DropShardKeyRequest,
    },
    ListShardKeys {
        collection: String,
    },
    ListCollections,
    GetCollection {
        collection: String,
    },
    /// Client-side cross-encoder: run candidate queries, score pairs, reorder.
    CrossRerank {
        collection: String,
        query: String,
        model: String,
        /// Payload field holding document text for pair scoring.
        field: String,
        limit: u64,
        offset: u64,
        /// Candidate ANN stages already planned as normal queries.
        candidates: Vec<(String, QueryRequest)>,
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
            PlannedOperation::Upsert { .. } => "UPSERT",
            PlannedOperation::Delete { .. } => "DELETE",
            PlannedOperation::UpdatePayload { .. } => "UPDATE_PAYLOAD",
            PlannedOperation::ClearPayload { .. } => "CLEAR_PAYLOAD",
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
            | PlannedOperation::Upsert { collection, .. }
            | PlannedOperation::Delete { collection, .. }
            | PlannedOperation::UpdatePayload { collection, .. }
            | PlannedOperation::ClearPayload { collection, .. }
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
            PlannedOperation::ListCollections => None,
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
            | PlannedOperation::UpdateVectors { .. }
            | PlannedOperation::DeleteVectors { .. } => BatchFamily::Mutation,
            // Pair scoring is not batchable with plain queries.
            PlannedOperation::CrossRerank { .. } => BatchFamily::Single,
            _ => BatchFamily::Single,
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
            PlannedOperation::Upsert { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::Delete { request, .. } => request.shard_key.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchFamily {
    Query,
    Mutation,
    Single,
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
                qql_core::ast::QueryCollection::Explicit(name) => name.clone(),
                qql_core::ast::QueryCollection::Inherited => String::new(),
            };
            let filter = count
                .filter
                .as_ref()
                .map(|f| crate::filter::top_level_filter(f));
            Ok(PlannedOperation::Count {
                collection,
                request: CountRequest {
                    filter,
                    shard_key: count.shard_key.clone(),
                    exact: None,
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
    }
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
                let cte = outer.ctes.iter().find(|c| c.name.eq_ignore_ascii_case(name));
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

/// REST projection of a planned operation (HTTP method/path/query/body).
pub fn to_rest_route(op: &PlannedOperation) -> Route {
    match op {
        PlannedOperation::Query {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query"),
            query: Vec::new(),
            body: Some(RequestBody::Query(Box::new(request.clone()))),
        },
        PlannedOperation::QueryGroups {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query/groups"),
            query: Vec::new(),
            body: Some(RequestBody::QueryGroups(Box::new(request.clone()))),
        },
        PlannedOperation::GetPoints {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points"),
            query: Vec::new(),
            body: Some(RequestBody::Points(request.clone())),
        },
        PlannedOperation::Scroll {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/scroll"),
            query: Vec::new(),
            body: Some(RequestBody::Scroll(Box::new(request.clone()))),
        },
        PlannedOperation::Count {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/count"),
            query: Vec::new(),
            body: Some(RequestBody::Count(Box::new(request.clone()))),
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
                body: Some(RequestBody::Upsert(request.clone())),
            }
        }
        PlannedOperation::Delete {
            collection,
            request,
        } => {
            let mut query = vec![("wait".into(), "true".into())];
            if let Some(ref sk) = request.shard_key {
                query.push(("shard_key".into(), sk.clone()));
            }
            Route {
                method: Method::Post,
                path: format!("/collections/{collection}/points/delete"),
                query,
                body: Some(RequestBody::Delete(Box::new(request.clone()))),
            }
        }
        PlannedOperation::ClearPayload {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload/clear"),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::ClearPayload(Box::new(request.clone()))),
        },
        PlannedOperation::DeleteVectors {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/vectors/delete"),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::DeleteVector(Box::new(request.clone()))),
        },
        PlannedOperation::UpdateVectors {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/points/vectors"),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::UpdateVector(request.clone())),
        },
        PlannedOperation::UpdatePayload {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload"),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::UpdatePayload(request.clone())),
        },
        PlannedOperation::CreateCollection {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: Some(RequestBody::CreateCollection(Box::new(request.clone()))),
        },
        PlannedOperation::UpdateCollection {
            collection,
            request,
        } => Route {
            method: Method::Patch,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: Some(RequestBody::UpdateCollection(Box::new(request.clone()))),
        },
        PlannedOperation::DropCollection { collection } => Route {
            method: Method::Delete,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::CreateIndex {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/index"),
            query: Vec::new(),
            body: Some(RequestBody::CreateIndex(request.clone())),
        },
        PlannedOperation::DropIndex { collection, field } => Route {
            method: Method::Delete,
            path: format!("/collections/{collection}/index/{field}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::CreateShardKey {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/shards"),
            query: Vec::new(),
            body: Some(RequestBody::CreateShardKey(Box::new(request.clone()))),
        },
        PlannedOperation::DropShardKey {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/shards/delete"),
            query: Vec::new(),
            body: Some(RequestBody::DropShardKey(Box::new(request.clone()))),
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
        // CrossRerank is client-side; never projected as a single Qdrant route.
        PlannedOperation::CrossRerank { collection, .. } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query"),
            query: Vec::new(),
            body: None,
        },
    }
}

/// Compatibility: plan + REST projection. Returns a planning error as a
/// validation failure rather than panicking on malformed programmatic AST.
pub fn try_route(statement: &Stmt) -> Result<Route, QqlError> {
    plan(statement).map(|op| to_rest_route(&op))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;

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
        let stmt = Parser::parse("QUERY 'hello' FROM docs LIMIT 5;").unwrap();
        let op = plan(&stmt).unwrap();
        let route = to_rest_route(&op);
        assert_eq!(route.path, "/collections/docs/points/query");
        assert!(matches!(route.body, Some(RequestBody::Query(_))));
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
        assert!(matches!(
            alter_route.body,
            Some(RequestBody::UpdateCollection(_))
        ));
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
}
