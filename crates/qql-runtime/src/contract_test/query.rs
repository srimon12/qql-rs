//! OpenAPI contract for every Qdrant-backed query variant.

use super::{openapi_or_skip, validate_ref};
use qql_core::parser::Parser;
use qql_plan::PlannedOperation;
use qql_plan::plan::{plan, to_rest_route};
use qql_plan::routing::try_route;

#[test]
fn test_contract_all_query_variants_match_openapi_json() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };

    let query_validator = jsonschema::validator_for(&serde_json::json!({
        "$ref": "#/components/schemas/Query",
        "components": openapi["components"]
    }))
    .expect("failed to compile Query schema from openapi.json");

    // Core variants + multi / hybrid / formula / groups-related nearest.
    let query_cases: &[(&str, &str)] = &[
        ("sample", "QUERY SAMPLE RANDOM FROM docs LIMIT 10;"),
        (
            "nearest text",
            "QUERY TEXT 'stroke' MODEL 'e5' FROM docs LIMIT 10;",
        ),
        (
            "nearest vector",
            "QUERY NEAREST VECTOR [0.1, 0.2] FROM docs USING dense LIMIT 5;",
        ),
        (
            "nearest multi-dense",
            "QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]] FROM docs USING colbert LIMIT 5;",
        ),
        (
            "nearest point",
            "QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 5;",
        ),
        (
            "recommend",
            "QUERY RECOMMEND POSITIVE (1) NEGATIVE (2) STRATEGY average_vector FROM docs USING dense LIMIT 10;",
        ),
        (
            "context",
            "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs LIMIT 10;",
        ),
        (
            "discover",
            "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;",
        ),
        (
            "relevance feedback",
            "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5), (POINT 44, -0.2)) STRATEGY NAIVE (a = 1.0, b = 0.5, c = 0.5) FROM docs USING dense LIMIT 10;",
        ),
        (
            "order_by",
            "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
        ),
        (
            "fusion",
            "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 10;",
        ),
        (
            "hybrid",
            "QUERY HYBRID TEXT 'x' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        ),
        (
            "using hybrid",
            "QUERY TEXT 'x' MODEL 'bge' FROM docs USING HYBRID DENSE dense SPARSE sparse FUSION DBSF LIMIT 10;",
        ),
        (
            "mmr",
            "QUERY MMR TEXT 'x' MODEL 'embedder' DIVERSITY 0.4 CANDIDATES 100 FROM docs USING dense LIMIT 5;",
        ),
        (
            "rerank late-interaction",
            "QUERY RERANK TEXT 'travel' MODEL 'colbert' FROM docs USING colbert PREFETCH (QUERY TEXT 'travel' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
        ),
        (
            "formula",
            "QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
        ),
        (
            "formula max/min",
            "QUERY FORMULA MAX(score * 2.0, MIN(score, bonus)) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
        ),
        (
            "formula max/min single operand",
            // n ≥ 1: a one-term fold is valid QQL and must satisfy the
            // OpenAPI MaxExpression / MinExpression oneOf members.
            "QUERY FORMULA MAX(score) + MIN(1.0) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
        ),
        (
            "formula acosh",
            "QUERY FORMULA ACOSH(1.0 + score) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
        ),
    ];

    for (name, qql) in query_cases {
        let stmt = Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {name}: {e}"));
        let r = try_route(&stmt).unwrap();
        let json = r
            .body_json()
            .unwrap_or_else(|| panic!("no body for {name}"));
        let query = json
            .get("query")
            .unwrap_or_else(|| panic!("no query field in body for {name}: {json}"));

        let errors: Vec<_> = query_validator.iter_errors(query).collect();
        assert!(
            errors.is_empty(),
            "Contract Violation: {name} query failed openapi.json schema validation: {errors:?}\nQuery JSON: {query}"
        );
    }

    // Multi-dense nearest must serialize as array-of-arrays under OpenAPI Query.
    {
        let stmt = Parser::parse(
            "QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4]] FROM docs USING colbert LIMIT 5;",
        )
        .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        let nearest = &json["query"]["nearest"];
        assert!(
            nearest.as_array().is_some_and(|rows| {
                rows.len() == 2 && rows[0].as_array().is_some_and(|r| r.len() == 2)
            }),
            "multi-dense nearest must be array-of-arrays, got {nearest}"
        );
        validate_ref(&openapi, "Query", &json["query"]);
    }

    // Hybrid expands to fusion + two prefetches — body is QueryRequest-shaped.
    {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'bge' FROM docs USING HYBRID DENSE dense SPARSE sparse LIMIT 10;",
        )
        .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        assert_eq!(json["query"]["fusion"], "rrf");
        assert_eq!(json["prefetch"].as_array().unwrap().len(), 2);
        validate_ref(&openapi, "QueryRequest", &json);
    }

    // Groups request (no offset) validates as QueryGroupsRequest.
    {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense GROUP BY topic SIZE 3 LIMIT 10;",
        )
        .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        assert!(json.get("offset").is_none());
        assert_eq!(json["group_by"], "topic");
        validate_ref(&openapi, "QueryGroupsRequest", &json);
    }

    // Request-level timeout/consistency are query params, not body fields.
    {
        let stmt = Parser::parse(
                "QUERY VECTOR [0.1, 0.2] FROM docs USING dense PARAMS (timeout = 30, consistency = majority) LIMIT 5;",
            )
            .unwrap();
        let op = plan(&stmt).unwrap();
        let r = to_rest_route(&op).expect("rest route");
        assert!(r.query.iter().any(|(k, v)| k == "timeout" && v == "30"));
        assert!(
            r.query
                .iter()
                .any(|(k, v)| k == "consistency" && v == "majority")
        );
        let body = r.body_json().unwrap();
        assert!(body.get("timeout").is_none());
        assert!(body.get("consistency").is_none());
        validate_ref(&openapi, "QueryRequest", &body);
    }

    // CROSS RERANK is not a Qdrant Query body — plan only.
    {
        let stmt = Parser::parse(
            "WITH c AS (QUERY TEXT 'q' MODEL 'test-model' FROM docs USING dense LIMIT 50) \
                 QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD text \
                 FROM docs PREFETCH (c) LIMIT 10;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();
        assert!(
            matches!(op, PlannedOperation::CrossRerank { .. }),
            "CROSS RERANK must plan as CrossRerank, got {op:?}"
        );
        assert!(
            to_rest_route(&op).is_err(),
            "CROSS RERANK must not invent a Qdrant REST route"
        );
    }

    // IMAGE is embed-time; plan with unresolved IMAGE still lowers to document-like
    // text path only after prepare. Contract: precomputed dense after IMAGE is Query.
    {
        // After embed, IMAGE becomes dense VECTOR — contract the dense path.
        let stmt =
            Parser::parse("QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING image LIMIT 5;")
                .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        validate_ref(&openapi, "Query", &json["query"]);
    }

    // Filters
    let filter_validator = jsonschema::validator_for(&serde_json::json!({
        "$ref": "#/components/schemas/Filter",
        "components": openapi["components"]
    }))
    .expect("failed to compile Filter schema from openapi.json");

    let filter_cases: &[(&str, &str)] = &[
        (
            "equality",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE status = 'active';",
        ),
        (
            "inequality range",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE age >= 21 AND score < 100.0;",
        ),
        (
            "between",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE age BETWEEN 20 AND 30;",
        ),
        (
            "in list",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE tag IN ('a', 'b', 'c');",
        ),
        (
            "is null",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE deleted_at IS NULL;",
        ),
        (
            "is empty",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE tags IS EMPTY;",
        ),
        (
            "match text",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE body MATCH 'hello world';",
        ),
        (
            "match phrase",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE body MATCH PHRASE 'hello world';",
        ),
        (
            "match any",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE body MATCH ANY ('hello', 'world');",
        ),
        (
            "match prefix",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE title MATCH PREFIX 'Comp';",
        ),
        (
            "has vector",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE HAS_VECTOR 'dense';",
        ),
        (
            "slice",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE SLICE (4, 1);",
        ),
        (
            "values count",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE tags VALUES_COUNT >= 2;",
        ),
        (
            "nested",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE NESTED('reviews', rating > 4);",
        ),
        (
            "geo bbox",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE location GEO_BBOX { top_left: {lat: 52.52, lon: 13.40}, bottom_right: {lat: 52.51, lon: 13.41} };",
        ),
        (
            "geo radius",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE location GEO_RADIUS { center: {lat: 52.52, lon: 13.40}, radius: 1000.0 };",
        ),
        (
            "geo polygon",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE location GEO_POLYGON { exterior: [{lat: -70.0, lon: -70.0}, {lat: 60.0, lon: -70.0}, {lat: 60.0, lon: 60.0}, {lat: -70.0, lon: 60.0}] } ;",
        ),
        (
            "point id eq",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE id = 42;",
        ),
        (
            "point id in",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE id IN (1, 2, 3);",
        ),
        (
            "compound or not",
            "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE (status = 'a' OR status = 'b') AND NOT category = 'c';",
        ),
    ];

    for (name, qql) in filter_cases {
        let stmt = Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {name}: {e}"));
        let r = try_route(&stmt).unwrap();
        let json = r
            .body_json()
            .unwrap_or_else(|| panic!("no body for {name}"));
        let filter = json
            .get("filter")
            .unwrap_or_else(|| panic!("no filter field in body for {name}: {json}"));

        let norm_filter = if filter.get("must").is_none()
            && filter.get("should").is_none()
            && filter.get("must_not").is_none()
        {
            serde_json::json!({ "must": [filter] })
        } else {
            filter.clone()
        };

        let errors: Vec<_> = filter_validator.iter_errors(&norm_filter).collect();
        assert!(
            errors.is_empty(),
            "Contract Violation: {name} filter failed openapi.json schema validation: {errors:?}\nFilter JSON: {norm_filter}"
        );
    }

    let scroll_stmt = Parser::parse("SCROLL FROM docs WHERE status = 'active' LIMIT 50;").unwrap();
    let scroll_json = try_route(&scroll_stmt).unwrap().body_json().unwrap();
    validate_ref(&openapi, "ScrollRequest", &scroll_json);

    let points_stmt =
        Parser::parse("QUERY POINTS (42, 'uuid-v4') FROM docs WITH PAYLOAD INCLUDE ('title');")
            .unwrap();
    let points_json = try_route(&points_stmt).unwrap().body_json().unwrap();
    validate_ref(&openapi, "PointRequest", &points_json);
}
