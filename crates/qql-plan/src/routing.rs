use crate::ddl::{lower_alter_collection, lower_create_collection, lower_create_index};
use crate::mutation::{
    lower_clear_payload_request, lower_delete_request, lower_delete_vector_request,
    lower_scroll_request, lower_update_payload_request, lower_update_vector_request,
    lower_upsert_request,
};
use crate::query::lower_query_request;
use crate::types::*;
use qql_core::ast::{QueryCollection, QueryExpr, QueryStmt, Stmt};

#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum RequestBody {
    Query(Box<QueryRequest>),
    QueryGroups(Box<QueryGroupsRequest>),
    Points(PointsRequest),
    Scroll(Box<ScrollRequest>),
    Upsert(UpsertRequest),
    Delete(Box<DeleteRequest>),
    ClearPayload(Box<ClearPayloadRequest>),
    DeleteVector(Box<DeleteVectorRequest>),
    UpdateVector(UpdateVectorRequest),
    UpdatePayload(UpdatePayloadRequest),
    CreateCollection(Box<CreateCollectionRequest>),
    CreateIndex(CreateIndexRequest),
    Count(Box<CountRequest>),
    CreateShardKey(Box<CreateShardKeyRequest>),
}

impl RequestBody {
    pub fn to_json(&self) -> Result<serde_json::Value, qql_core::error::QqlError> {
        serde_json::to_value(self).map_err(|err| {
            qql_core::error::QqlError::validation(
                "QQL-PLAN-SERIALIZE",
                alloc::format!("failed to serialize request body: {err}"),
                None,
            )
        })
    }
}

pub struct Route {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<RequestBody>,
}

impl Route {
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body.as_ref().and_then(|b| b.to_json().ok())
    }
}

pub fn route(statement: &Stmt) -> Route {
    match statement {
        Stmt::Query(query) => {
            let collection = match &query.collection {
                QueryCollection::Explicit(name) => name.clone(),
                QueryCollection::Inherited => String::new(),
            };

            if matches!(query.expression, QueryExpr::Points { .. }) {
                // ... points lookup (same as before) ...
                let ids = match &query.expression {
                    QueryExpr::Points { ids } => ids,
                    _ => unreachable!(),
                };
                let id_list: Vec<_> = ids
                    .iter()
                    .map(|id| match id {
                        qql_core::ast::PointId::Number(n) => serde_json::Value::Number((*n).into()),
                        qql_core::ast::PointId::String(s) => serde_json::Value::String(s.clone()),
                    })
                    .collect();
                let with_payload = query.output.payload.as_ref().map(|p| match p {
                    qql_core::ast::PayloadSelector::All => PayloadSelectorReq::All(true),
                    qql_core::ast::PayloadSelector::None => PayloadSelectorReq::All(false),
                    qql_core::ast::PayloadSelector::Include(fields) => {
                        PayloadSelectorReq::Include {
                            include: fields.clone(),
                        }
                    }
                    qql_core::ast::PayloadSelector::Exclude(fields) => {
                        PayloadSelectorReq::Exclude {
                            exclude: fields.clone(),
                        }
                    }
                });
                let with_vector = query.output.vectors.as_ref().map(|v| match v {
                    qql_core::ast::VectorSelector::All => VectorSelectorReq::All(true),
                    qql_core::ast::VectorSelector::None => VectorSelectorReq::All(false),
                    qql_core::ast::VectorSelector::Names(names) => {
                        VectorSelectorReq::Names(names.clone())
                    }
                });

                Route {
                    method: Method::Post,
                    path: format!("/collections/{}/points", collection),
                    query: Vec::new(),
                    body: Some(RequestBody::Points(PointsRequest {
                        ids: id_list,
                        with_payload,
                        with_vector,
                    })),
                }
            } else if query.group.is_some() {
                let req = crate::query::lower_query_groups_request(query);
                Route {
                    method: Method::Post,
                    path: format!("/collections/{}/points/query/groups", collection),
                    query: Vec::new(),
                    body: Some(RequestBody::QueryGroups(Box::new(req))),
                }
            } else {
                Route {
                    method: Method::Post,
                    path: format!("/collections/{}/points/query", collection),
                    query: Vec::new(),
                    body: Some(RequestBody::Query(Box::new(lower_query_request(query)))),
                }
            }
        }
        Stmt::Scroll(scroll) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/scroll", scroll.collection),
            query: Vec::new(),
            body: Some(RequestBody::Scroll(Box::new(lower_scroll_request(
                scroll.limit,
                scroll.filter.as_deref(),
                scroll.after.as_ref(),
                scroll.shard_key.clone(),
            )))),
        },
        Stmt::Upsert(upsert) => {
            let mut query = Vec::new();
            if upsert.embedding.is_some() {
                query.push(("wait".into(), "true".into()));
            }
            if let Some(ref shard_key) = upsert.shard_key {
                query.push(("shard_key".into(), shard_key.clone()));
            }
            Route {
                method: Method::Put,
                path: format!("/collections/{}/points", upsert.collection),
                query,
                body: Some(RequestBody::Upsert(lower_upsert_request(upsert))),
            }
        }
        Stmt::Delete(delete) => {
            let mut query = vec![("wait".into(), "true".into())];
            if let Some(ref shard_key) = delete.shard_key {
                query.push(("shard_key".into(), shard_key.clone()));
            }
            Route {
                method: Method::Post,
                path: format!("/collections/{}/points/delete", delete.collection),
                query,
                body: Some(RequestBody::Delete(Box::new(lower_delete_request(delete)))),
            }
        }
        Stmt::ClearPayload(clear) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/payload/clear", clear.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::ClearPayload(Box::new(
                lower_clear_payload_request(clear),
            ))),
        },
        Stmt::DeleteVector(del_vec) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/vectors/delete", del_vec.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::DeleteVector(Box::new(
                lower_delete_vector_request(del_vec),
            ))),
        },
        Stmt::UpdateVector(update) => Route {
            method: Method::Put,
            path: format!("/collections/{}/points/vectors", update.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::UpdateVector(lower_update_vector_request(
                update,
            ))),
        },
        Stmt::UpdatePayload(update) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/payload", update.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::UpdatePayload(lower_update_payload_request(
                update,
            ))),
        },
        Stmt::CreateCollection(create) => Route {
            method: Method::Put,
            path: format!("/collections/{}", create.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateCollection(Box::new(
                lower_create_collection(create),
            ))),
        },
        Stmt::AlterCollection(alter) => Route {
            method: Method::Patch,
            path: format!("/collections/{}", alter.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateCollection(Box::new(
                lower_alter_collection(alter),
            ))),
        },
        Stmt::DropCollection(drop) => Route {
            method: Method::Delete,
            path: format!("/collections/{}", drop.collection),
            query: Vec::new(),
            body: None,
        },
        Stmt::CreateIndex(index) => Route {
            method: Method::Put,
            path: format!("/collections/{}/index", index.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateIndex(lower_create_index(index))),
        },
        Stmt::DropIndex(index) => Route {
            method: Method::Delete,
            path: format!("/collections/{}/index/{}", index.collection, index.field),
            query: Vec::new(),
            body: None,
        },
        Stmt::Count(count) => {
            let filter = count
                .filter
                .as_ref()
                .map(|f| crate::filter::top_level_filter(f));
            Route {
                method: Method::Post,
                path: format!("/collections/{}/points/count", count.collection),
                query: Vec::new(),
                body: Some(RequestBody::Count(Box::new(CountRequest {
                    filter,
                    shard_key: count.shard_key.clone(),
                    exact: None,
                }))),
            }
        }
        Stmt::CreateShardKey(sk) => Route {
            method: Method::Put,
            path: format!("/collections/{}/shards", sk.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateShardKey(Box::new(
                CreateShardKeyRequest {
                    shard_key: sk.shard_key.clone(),
                    shards_number: sk.shards_number,
                    replication_factor: sk.replication_factor,
                },
            ))),
        },
        Stmt::ShowCollections => Route {
            method: Method::Get,
            path: "/collections".into(),
            query: Vec::new(),
            body: None,
        },
        Stmt::ShowCollection(collection) => Route {
            method: Method::Get,
            path: format!("/collections/{}", collection),
            query: Vec::new(),
            body: None,
        },
    }
}

/// Groups QUERY statements by collection and produces one `QueryBatchRequest`
/// per unique collection.  Only standard queries (non-`Points`, non-grouped)
/// are batched.  Returns an empty vec when fewer than 2 queries share a
/// collection — single queries use the normal `route()` path.
pub fn route_query_batch(stmts: &[QueryStmt]) -> Vec<(String, QueryBatchRequest)> {
    let mut groups: std::collections::HashMap<String, Vec<QueryRequest>> =
        std::collections::HashMap::new();

    for stmt in stmts {
        // Skip non-batchable variants — Points lookups and grouped queries
        // use different Qdrant endpoints.
        if matches!(stmt.expression, QueryExpr::Points { .. }) || stmt.group.is_some() {
            continue;
        }
        let collection = match &stmt.collection {
            QueryCollection::Explicit(name) => name.clone(),
            QueryCollection::Inherited => continue,
        };
        groups
            .entry(collection)
            .or_default()
            .push(lower_query_request(stmt));
    }

    groups
        .into_iter()
        .filter(|(_, reqs)| reqs.len() > 1) // single queries use the normal route
        .map(|(collection, searches)| (collection, QueryBatchRequest { searches }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;

    #[test]
    fn query_routes_correctly() {
        let s = Parser::parse("QUERY 'hello' FROM docs LIMIT 10;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query");
        assert!(r.body.is_some());
    }

    #[test]
    fn points_lookup() {
        let s = Parser::parse("QUERY POINTS (42) FROM docs WITH PAYLOAD true;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points");
    }

    #[test]
    fn upsert_with_embedding_waits() {
        let s = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'x'} USING DENSE MODEL 'm';")
            .unwrap();
        let r = route(&s);
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn delete_has_wait() {
        let s = Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
        let r = route(&s);
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn show_collections_no_body() {
        let s = Parser::parse("SHOW COLLECTIONS;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Get);
        assert!(r.body.is_none());
    }

    #[test]
    fn all_endpoint_methods() {
        let cases = [
            (
                "QUERY 'x' FROM docs;",
                Method::Post,
                "/collections/docs/points/query",
            ),
            (
                "SCROLL FROM docs LIMIT 10;",
                Method::Post,
                "/collections/docs/points/scroll",
            ),
            (
                "UPSERT INTO docs VALUES {id: 1, title: 'x'};",
                Method::Put,
                "/collections/docs/points",
            ),
            (
                "DELETE FROM docs WHERE id = 1;",
                Method::Post,
                "/collections/docs/points/delete",
            ),
            (
                "UPDATE docs SET VECTOR = [0.1] WHERE id = 'x';",
                Method::Put,
                "/collections/docs/points/vectors",
            ),
            (
                "UPDATE docs SET PAYLOAD = {x: 1} WHERE id = 1;",
                Method::Post,
                "/collections/docs/points/payload",
            ),
            (
                "CREATE COLLECTION docs (d VECTOR(4, DOT));",
                Method::Put,
                "/collections/docs",
            ),
            (
                "ALTER COLLECTION docs WITH HNSW (m = 16);",
                Method::Patch,
                "/collections/docs",
            ),
            ("DROP COLLECTION docs;", Method::Delete, "/collections/docs"),
            (
                "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
                Method::Put,
                "/collections/docs/index",
            ),
            ("SHOW COLLECTIONS;", Method::Get, "/collections"),
            ("SHOW COLLECTION docs;", Method::Get, "/collections/docs"),
        ];
        for (source, method, path) in cases {
            let s = Parser::parse(source).unwrap();
            let r = route(&s);
            assert_eq!(r.method, method, "method mismatch for: {}", source);
            assert_eq!(r.path, path, "path mismatch for: {}", source);
        }
    }

    #[test]
    fn grouped_query_routes_to_groups_endpoint() {
        let s =
            Parser::parse("QUERY 'hello' FROM docs GROUP BY category SIZE 3 LIMIT 10;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query/groups");
        assert!(r.body.is_some());
    }

    #[test]
    fn grouped_query_with_lookup() {
        let s = Parser::parse(
            "QUERY 'hello' FROM docs GROUP BY category SIZE 3 LOOKUP FROM categories LIMIT 10;",
        )
        .unwrap();
        let r = route(&s);
        assert_eq!(r.path, "/collections/docs/points/query/groups");
        let json = r.body_json().unwrap();
        assert_eq!(json["group_by"], "category");
        assert_eq!(json["group_size"], 3);
        assert_eq!(json["with_lookup"], "categories");
    }

    #[test]
    fn hybrid_query_produces_prefetches() {
        let s = Parser::parse(
            "QUERY HYBRID TEXT 'database' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
        let r = route(&s);
        let json = r.body_json().unwrap();
        assert_eq!(json["query"]["fusion"], "rrf");
        let prefetch = json["prefetch"].as_array().unwrap();
        assert_eq!(prefetch.len(), 2);
        assert_eq!(prefetch[0]["using"], "dense");
        assert_eq!(prefetch[1]["using"], "sparse");
        assert!(
            prefetch[0]["query"]["nearest"].is_object()
                || prefetch[0]["query"]["nearest"].is_string()
        );
    }

    #[test]
    fn rerank_query_staged() {
        let s = Parser::parse(
            "QUERY RERANK TEXT 'travel' MODEL 'colbert' FROM docs USING colbert PREFETCH (QUERY 'travel' FROM docs USING dense LIMIT 100) LIMIT 10;",
        )
        .unwrap();
        let r = route(&s);
        let json = r.body_json().unwrap();
        assert_eq!(json["using"], "colbert");
        assert!(json["query"]["nearest"].is_object());
        assert_eq!(json["prefetch"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn points_lookup_full() {
        let s = Parser::parse(
            "QUERY POINTS (42, 'uuid-v4') FROM docs WITH PAYLOAD INCLUDE ('title', 'url') WITH VECTOR ('dense');",
        )
        .unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points");
        let json = r.body_json().unwrap();
        assert_eq!(json["ids"].as_array().unwrap().len(), 2);
        assert_eq!(json["with_payload"]["include"].as_array().unwrap().len(), 2);
        assert_eq!(json["with_vector"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn query_with_all_options() {
        let s = Parser::parse(
            "QUERY 'search' FROM docs USING dense WHERE status = 'active' PARAMS (hnsw_ef = 256, exact = true) SCORE THRESHOLD 0.5 WITH PAYLOAD INCLUDE ('title') WITH VECTOR ('dense') LIMIT 20 OFFSET 5;",
        )
        .unwrap();
        let r = route(&s);
        let json = r.body_json().unwrap();
        assert_eq!(json["query"]["nearest"], "search");
        assert_eq!(json["using"], "dense");
        assert_eq!(json["filter"]["must"][0]["key"], "status");
        assert_eq!(json["filter"]["must"][0]["match"]["value"], "active");
        assert_eq!(json["params"]["hnsw_ef"], 256);
        assert_eq!(json["params"]["exact"], true);
        assert_eq!(json["score_threshold"], 0.5);
        assert_eq!(json["limit"], 20);
        assert_eq!(json["offset"], 5);
    }

    #[test]
    fn scroll_with_order_by() {
        let s = Parser::parse("SCROLL FROM docs WHERE status = 'active' LIMIT 50;").unwrap();
        let r = route(&s);
        let json = r.body_json().unwrap();
        assert_eq!(json["with_payload"], true);
        assert!(json["limit"].as_u64().unwrap() > 0);
    }

    #[test]
    fn query_body_has_no_group_fields_when_no_group() {
        let s = Parser::parse("QUERY 'hello' FROM docs LIMIT 5;").unwrap();
        let r = route(&s);
        let json = r.body_json().unwrap();
        assert!(json.get("group_by").is_none());
        assert!(json.get("group_size").is_none());
        assert!(json.get("group_request").is_none());
    }

    #[test]
    fn query_body_serialization_roundtrip_all_variants() {
        let cases = [
            "QUERY 'text search' FROM docs LIMIT 10;",
            "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 10;",
            "QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 5;",
            "QUERY NEAREST POINT '550e8400-e29b-41d4-a716-446655440000' FROM docs USING dense;",
            "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY average_vector FROM docs USING dense LIMIT 10;",
            "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs LIMIT 10;",
            "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;",
            "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
            "QUERY SAMPLE RANDOM FROM docs LIMIT 10;",
            "QUERY FORMULA score * 2 FROM docs LIMIT 5;",
            "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 1, 0.8), (POINT 2, 0.2)) STRATEGY NAIVE (a = 1.0, b = 1.0, c = 1.0) FROM docs USING dense LIMIT 10;",
            "UPSERT INTO docs VALUES {id: 1, title: 'hello'};",
            "DELETE FROM docs WHERE status = 'inactive';",
            "UPDATE docs SET VECTOR = [0.1, 0.2] WHERE id = 1;",
            "UPDATE docs SET PAYLOAD = {x: 1} WHERE id = 1;",
            "CREATE COLLECTION docs (d VECTOR(128, COSINE));",
            "ALTER COLLECTION docs WITH HNSW (m = 16);",
            "DROP COLLECTION docs;",
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
            "SHOW COLLECTIONS;",
            "SHOW COLLECTION docs;",
            "SCROLL FROM docs LIMIT 10;",
        ];
        for source in cases {
            let s = Parser::parse(source).unwrap_or_else(|_| panic!("parse failed: {}", source));
            let r = route(&s);
            let json = r.body_json();
            match r.body {
                Some(_) => assert!(json.is_some(), "expected body for: {}", source),
                None => assert!(json.is_none(), "expected no body for: {}", source),
            }
        }
    }
}
