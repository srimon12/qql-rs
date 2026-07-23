use crate::plan::{plan, to_rest_route};
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
    /// PATCH alter collection — distinct from create (PUT).
    UpdateCollection(Box<UpdateCollectionRequest>),
    CreateIndex(CreateIndexRequest),
    Count(Box<CountRequest>),
    CreateShardKey(Box<CreateShardKeyRequest>),
    DropShardKey(Box<DropShardKeyRequest>),
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
    pub fn try_body_json(&self) -> Result<Option<serde_json::Value>, qql_core::error::QqlError> {
        self.body.as_ref().map(RequestBody::to_json).transpose()
    }

    /// Compatibility helper for callers that only need an optional body.
    /// Request bodies contain JSON-safe values, so serialization failure is an
    /// internal invariant violation rather than an absent body.
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.try_body_json()
            .expect("request body serialization must be infallible")
    }
}

/// Compatibility REST projection. Prefer [`crate::plan::plan`] +
/// [`crate::plan::to_rest_route`] (or [`crate::plan::try_route`]) for new code.
///
/// Parser-validated statements always succeed. Programmatic malformed AST
/// returns a planning error via `try_route`; this infallible wrapper falls
/// back to an empty GET only for unexpected validation failures (should not
/// occur for parser-produced statements).
pub fn route(statement: &Stmt) -> Route {
    match plan(statement) {
        Ok(op) => to_rest_route(&op),
        Err(_) => Route {
            method: Method::Get,
            path: String::new(),
            query: Vec::new(),
            body: None,
        },
    }
}

/// Fallible route construction (planner + REST projection).
pub fn try_route(statement: &Stmt) -> Result<Route, qql_core::error::QqlError> {
    crate::plan::try_route(statement)
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
        if let Ok(req) = lower_query_request(stmt) {
            groups.entry(collection).or_default().push(req);
        }
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
