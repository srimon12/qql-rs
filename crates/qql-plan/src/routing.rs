use crate::plan::{plan, to_rest_route};
use crate::types::*;
use qql_core::ast::Stmt;

/// Optional REST projection of a plan: HTTP method, path, query, and body.
#[derive(Debug)]
pub struct Route {
    /// HTTP verb of the projected route.
    pub method: Method,
    /// Absolute Qdrant path with the collection interpolated.
    pub path: String,
    /// Ordered query-string parameters as `(name, value)` pairs.
    pub query: Vec<(String, String)>,
    /// Serialized JSON body; `None` for bodyless routes.
    pub body: Option<serde_json::Value>,
}

impl Route {
    /// Serialized JSON body, or None for bodyless routes.
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body.clone()
    }
}

/// Fallible REST projection of a statement. Prefer for new code.
///
/// Client-side ops (CROSS RERANK) and plan failures return `Err` — never a
/// silent empty GET.
pub fn try_route(statement: &Stmt) -> Result<Route, qql_core::error::QqlError> {
    crate::plan::try_route(statement)
}

/// Offline compile result for host SDKs.
///
/// `route` is `None` for client-side operations (e.g. CROSS RERANK) that have
/// a stable `stmt_type` but no single Qdrant HTTP endpoint.
#[derive(Debug)]
pub struct CompiledStatement {
    /// Stable snake_case type id from `compile_stmt_type`.
    pub stmt_type: &'static str,
    /// Projected REST route; `None` for client-side-only operations.
    pub route: Option<Route>,
}

/// Compile a statement from the planner IR.
///
/// Always sets `stmt_type` from [`crate::plan::PlannedOperation::compile_stmt_type`].
/// REST path/method/payload are present only when a real Qdrant route exists.
pub fn compile_statement(statement: &Stmt) -> Result<CompiledStatement, qql_core::error::QqlError> {
    let op = plan(statement)?;
    let stmt_type = op.compile_stmt_type();
    let route = to_rest_route(&op).ok();
    Ok(CompiledStatement { stmt_type, route })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plan, PlannedOperation};
    use qql_core::parser::Parser;

    #[test]
    fn client_side_ops_error_via_try_route_and_compile_cleanly() {
        // Regression: the deprecated `route()` (panicking wrapper around
        // `try_route`) was removed. `try_route` must return `Err` — never panic —
        // for client-side-only ops, and `compile_statement` must still expose the
        // stable `stmt_type` with no route.
        let s = Parser::parse(
            "QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD body FROM docs PREFETCH (QUERY TEXT 'q' FROM docs USING dense LIMIT 50) LIMIT 10;",
        )
        .unwrap();
        assert!(
            try_route(&s).is_err(),
            "client-side op must not produce a REST route"
        );
        let compiled = compile_statement(&s).unwrap();
        assert_eq!(compiled.stmt_type, "cross_rerank");
        assert!(compiled.route.is_none());
    }

    #[test]
    fn query_routes_correctly() {
        let s = Parser::parse("QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 10;").unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query");
        assert!(r.body.is_some());
    }

    #[test]
    fn points_lookup() {
        let s = Parser::parse("QUERY POINTS (42) FROM docs WITH PAYLOAD true;").unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points");
    }

    #[test]
    fn points_lookup_preserves_cluster_shard_key() {
        let statement = Parser::parse("QUERY POINTS (42) FROM docs SHARD 'tenant-a';").unwrap();
        let operation = plan::plan(&statement).unwrap();
        let PlannedOperation::GetPoints { request, .. } = operation else {
            panic!("expected point lookup");
        };
        assert_eq!(request.shard_key.as_deref(), Some("tenant-a"));
        assert!(try_route(&statement)
            .unwrap()
            .body_json()
            .unwrap()
            .to_string()
            .contains("tenant-a"));
    }

    #[test]
    fn quota_routes() {
        let show = Parser::parse("SHOW QUOTAS;").unwrap();
        let r = try_route(&show).unwrap();
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.path, "/quotas");
        assert!(r.body.is_none());

        let set = Parser::parse(
            "SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;",
        )
        .unwrap();
        let r = try_route(&set).unwrap();
        assert_eq!(r.method, Method::Put);
        assert_eq!(r.path, "/quotas");
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
        let body = r.body_json().unwrap();
        assert_eq!(body["enabled"], true);
        assert_eq!(body["max_resident_memory_percent"], 80);
        assert!(body.get("wait").is_none(), "wait must be a query param");
    }

    #[test]
    fn quota_plan_validates_config() {
        use crate::plan::{plan, PlannedOperation};
        let bad = Parser::parse("SET QUOTA (bogus = 1);").unwrap();
        let err = plan(&bad).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-QUOTA");

        let bad_range = Parser::parse("SET QUOTA (max_resident_memory_percent = 500);").unwrap();
        assert!(plan(&bad_range).is_err());

        let clear = Parser::parse("SET QUOTA (max_disk_usage_percent = null);").unwrap();
        let op = plan(&clear).unwrap();
        match op {
            PlannedOperation::SetQuotas { request } => {
                assert_eq!(request.max_disk_usage_percent, None);
            }
            other => panic!("expected SetQuotas, got {other:?}"),
        }
    }

    #[test]
    fn compile_stmt_type_disambiguates_bodyless_routes() {
        let cases = [
            ("DROP INDEX ON COLLECTION docs FOR title;", "drop_index"),
            ("SHOW SHARD KEYS ON COLLECTION docs;", "show_shard_keys"),
            ("DROP COLLECTION docs;", "drop_collection"),
            ("SHOW COLLECTION docs;", "show_collection"),
            ("SHOW COLLECTIONS;", "show_collections"),
        ];
        for (qql, expected) in cases {
            let stmt = Parser::parse(qql).unwrap();
            let compiled = compile_statement(&stmt).unwrap();
            assert_eq!(compiled.stmt_type, expected, "qql={qql}");
        }
    }

    #[test]
    fn mutation_shard_keys_lower_and_project() {
        let cases = [
            ("CLEAR PAYLOAD FROM docs WHERE id = 1 SHARD 't1';", "t1"),
            (
                "DELETE VECTOR dense FROM docs WHERE id = 1 SHARD 't2';",
                "t2",
            ),
            (
                "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1 SHARD 't3';",
                "t3",
            ),
            (
                "UPDATE docs SET PAYLOAD = {\"k\": 1} WHERE id = 1 SHARD 't4';",
                "t4",
            ),
        ];
        for (qql, expected) in cases {
            let statement = Parser::parse(qql).unwrap();
            let operation = plan::plan(&statement).unwrap();
            assert_eq!(
                operation.shard_key(),
                Some(expected),
                "plan.shard_key for {qql}"
            );
            let r = to_rest_route(&operation).expect("rest route");
            assert!(
                r.query
                    .iter()
                    .any(|(k, v)| k == "shard_key" && v == expected),
                "REST query param for {qql}: {:?}",
                r.query
            );
            let body = r.body_json().unwrap().to_string();
            assert!(
                body.contains(expected),
                "REST body should include shard_key for {qql}: {body}"
            );
        }
    }

    #[test]
    fn upsert_with_embedding_waits() {
        let s = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'x'} USING DENSE MODEL 'm';")
            .unwrap();
        let r = try_route(&s).unwrap();
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn delete_has_wait() {
        let s = Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
        let r = try_route(&s).unwrap();
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn show_collections_no_body() {
        let s = Parser::parse("SHOW COLLECTIONS;").unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Get);
        assert!(r.body.is_none());
    }

    #[test]
    fn all_endpoint_methods() {
        let cases = [
            (
                "QUERY TEXT 'x' MODEL 'e5' FROM docs;",
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
            let r = try_route(&s).unwrap();
            assert_eq!(r.method, method, "method mismatch for: {}", source);
            assert_eq!(r.path, path, "path mismatch for: {}", source);
        }
    }

    #[test]
    fn grouped_query_routes_to_groups_endpoint() {
        let s = Parser::parse(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs GROUP BY category SIZE 3 LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query/groups");
        assert!(r.body.is_some());
    }

    #[test]
    fn grouped_query_with_lookup() {
        let s = Parser::parse(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs GROUP BY category SIZE 3 LOOKUP FROM categories LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.path, "/collections/docs/points/query/groups");
        let json = r.body_json().unwrap();
        assert_eq!(json["group_by"], "category");
        assert_eq!(json["group_size"], 3);
        assert_eq!(json["with_lookup"], "categories");
    }

    #[test]
    fn hybrid_query_produces_prefetches() {
        let s = Parser::parse(
            "QUERY HYBRID TEXT 'database' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["query"]["fusion"], "rrf");
        let prefetch = json["prefetch"].as_array().unwrap();
        assert_eq!(prefetch.len(), 2);
        assert_eq!(prefetch[0]["using"], "dense");
        assert_eq!(prefetch[1]["using"], "sparse");
        assert!(
            prefetch[0]["query"]["nearest"].is_object(),
            "HYBRID prefetch nearest must be Document object: {:?}",
            prefetch[0]["query"]["nearest"]
        );
        assert!(
            prefetch[1]["query"]["nearest"].is_object(),
            "HYBRID sparse prefetch nearest must also be Document object: {:?}",
            prefetch[1]["query"]["nearest"]
        );
    }

    #[test]
    fn rerank_query_staged() {
        let s = Parser::parse(
            "QUERY RERANK TEXT 'travel' MODEL 'colbert' FROM docs USING colbert PREFETCH (QUERY TEXT 'travel' MODEL 'colbert' FROM docs USING dense LIMIT 100) LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
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
        let r = try_route(&s).unwrap();
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
            "QUERY TEXT 'search' MODEL 'e5' FROM docs USING dense WHERE status = 'active' PARAMS (hnsw_ef = 256, exact = true) SCORE THRESHOLD 0.5 WITH PAYLOAD INCLUDE ('title') WITH VECTOR ('dense') LIMIT 20 OFFSET 5;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert!(
            json["query"]["nearest"]["text"].is_string()
                || json["query"]["nearest"].as_str().is_some()
        );
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
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["with_payload"], true);
        assert_eq!(json["with_vector"], false);
        assert!(json["limit"].as_u64().unwrap() > 0);
    }

    #[test]
    fn scroll_with_vector_all() {
        let s = Parser::parse("SCROLL FROM docs WITH VECTOR LIMIT 25;").unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["with_payload"], true);
        assert_eq!(json["with_vector"], true);
        assert_eq!(json["limit"], 25);
    }

    #[test]
    fn scroll_with_vector_after_string_id() {
        let s = Parser::parse("SCROLL FROM docs AFTER 'id-with-quote' WITH VECTOR true LIMIT 10;")
            .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["offset"], "id-with-quote");
        assert_eq!(json["with_vector"], true);
    }

    #[test]
    fn query_body_has_no_group_fields_when_no_group() {
        let s = Parser::parse("QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 5;").unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert!(json.get("group_by").is_none());
        assert!(json.get("group_size").is_none());
        assert!(json.get("group_request").is_none());
    }

    #[test]
    fn query_body_serialization_roundtrip_all_variants() {
        let cases = [
            "QUERY TEXT 'text search' MODEL 'e5' FROM docs LIMIT 10;",
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
            let r = try_route(&s).unwrap();
            let json = r.body_json();
            match r.body {
                Some(_) => assert!(json.is_some(), "expected body for: {}", source),
                None => assert!(json.is_none(), "expected no body for: {}", source),
            }
        }
    }
}
