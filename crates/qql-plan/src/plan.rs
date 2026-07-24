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
use qql_core::ast::{QueryCollection, QueryExpr, Stmt};
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
            | PlannedOperation::GetCollection { collection } => Some(collection.as_str()),
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
            _ => BatchFamily::Single,
        }
    }

    /// Shard key carried on the plan, when present.
    pub fn shard_key(&self) -> Option<&str> {
        match self {
            PlannedOperation::Query { request, .. } => request.shard_key.as_deref(),
            PlannedOperation::QueryGroups { request, .. } => request.shard_key.as_deref(),
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
                    },
                });
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
            let filter = count
                .filter
                .as_ref()
                .map(|f| crate::filter::top_level_filter(f));
            Ok(PlannedOperation::Count {
                collection: count.collection.clone(),
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
        use qql_core::ast::{PageSpec, QueryInput, QueryOutput, QueryStmt};
        let stmt_empty_using = Stmt::Query(Box::new(QueryStmt {
            ctes: Vec::new(),
            collection: QueryCollection::Explicit("docs".into()),
            expression: QueryExpr::Rerank {
                input: QueryInput::Text {
                    text: "rerank text".into(),
                    model: None,
                },
                model: "colbert-v2".into(),
                using: String::new(),
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
                using: "dense".into(),
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
