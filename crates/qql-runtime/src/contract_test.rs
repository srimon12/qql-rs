//! OpenAPI contract tests + REST/gRPC parity checks for planned query bodies.
//!
//! Wire authority: `openapi.json` (REST body schemas) and `proto/points.proto`
//! (gRPC field mapping via `grpc_route` converters).

#[cfg(test)]
mod tests {
    use std::fs;

    use qql_core::parser::Parser;
    use qql_plan::plan::{plan, to_rest_route};
    use qql_plan::routing::try_route;
    use qql_plan::types::{PlanQueryInput, PlanVectorValue};
    use qql_plan::PlannedOperation;

    use crate::grpc_route::test_api;
    use crate::qdrant_grpc::qdrant;

    fn load_openapi_json() -> Option<serde_json::Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn openapi_or_skip() -> Option<serde_json::Value> {
        let openapi = load_openapi_json();
        if openapi.is_none() {
            eprintln!("openapi.json not found, skipping contract test");
        }
        openapi
    }

    fn validate_ref(openapi: &serde_json::Value, schema_name: &str, value: &serde_json::Value) {
        let validator = jsonschema::validator_for(&serde_json::json!({
            "$ref": format!("#/components/schemas/{schema_name}"),
            "components": openapi["components"]
        }))
        .unwrap_or_else(|e| panic!("failed to compile {schema_name} schema: {e}"));
        let errors: Vec<_> = validator.iter_errors(value).collect();
        assert!(
            errors.is_empty(),
            "OpenAPI {schema_name} violation: {errors:?}\nJSON: {value}"
        );
    }

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
            let stmt =
                Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {name}: {e}"));
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
            assert!(r
                .query
                .iter()
                .any(|(k, v)| k == "consistency" && v == "majority"));
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
            let stmt = Parser::parse(
                "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING image LIMIT 5;",
            )
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
                "has vector",
                "QUERY TEXT 'x' MODEL 'e5' FROM docs WHERE HAS_VECTOR 'dense';",
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
            let stmt =
                Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {name}: {e}"));
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

        let scroll_stmt =
            Parser::parse("SCROLL FROM docs WHERE status = 'active' LIMIT 50;").unwrap();
        let scroll_json = try_route(&scroll_stmt).unwrap().body_json().unwrap();
        validate_ref(&openapi, "ScrollRequest", &scroll_json);

        let points_stmt =
            Parser::parse("QUERY POINTS (42, 'uuid-v4') FROM docs WITH PAYLOAD INCLUDE ('title');")
                .unwrap();
        let points_json = try_route(&points_stmt).unwrap().body_json().unwrap();
        validate_ref(&openapi, "PointRequest", &points_json);
    }

    /// REST query params + body vs gRPC conversion field parity for key Query paths.
    #[test]
    fn rest_grpc_query_parity_timeout_consistency_shard_multi() {
        // MultiDense + timeout + consistency + shard_key
        let stmt = Parser::parse(
            "QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4]] FROM docs \
             USING colbert SHARD 'acme' \
             PARAMS (timeout = 15, consistency = quorum) LIMIT 7;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();
        let PlannedOperation::Query {
            collection,
            request,
        } = &op
        else {
            panic!("expected Query plan");
        };
        assert_eq!(collection, "docs");

        // REST projection
        let route = to_rest_route(&op).expect("rest route");
        assert_eq!(route.path, "/collections/docs/points/query");
        assert!(route.query.iter().any(|(k, v)| k == "timeout" && v == "15"));
        assert!(route
            .query
            .iter()
            .any(|(k, v)| k == "consistency" && v == "quorum"));
        let body = route.body_json().unwrap();
        assert_eq!(body["limit"], 7);
        assert_eq!(body["using"], "colbert");
        assert_eq!(body["shard_key"], "acme");
        assert!(body["query"]["nearest"].is_array());
        assert!(body.get("timeout").is_none());
        assert!(body.get("consistency").is_none());

        // gRPC conversion (proto QueryPoints)
        let grpc = test_api::to_query_points(request, collection).expect("to_query_points");
        assert_eq!(grpc.collection_name, "docs");
        assert_eq!(grpc.using.as_deref(), Some("colbert"));
        assert_eq!(grpc.limit, Some(7));
        assert_eq!(grpc.timeout, Some(15));
        assert!(grpc.read_consistency.is_some());
        let rc = grpc.read_consistency.as_ref().unwrap();
        use qdrant::read_consistency::Value as RcVal;
        match rc.value.as_ref() {
            Some(RcVal::Type(t)) => {
                assert_eq!(*t, qdrant::ReadConsistencyType::Quorum as i32);
            }
            other => panic!("expected quorum type, got {other:?}"),
        }
        assert!(grpc.shard_key_selector.is_some());
        let sk = &grpc.shard_key_selector.as_ref().unwrap().shard_keys;
        assert_eq!(sk.len(), 1);
        match sk[0].key.as_ref() {
            Some(qdrant::shard_key::Key::Keyword(k)) => assert_eq!(k, "acme"),
            other => panic!("expected keyword shard, got {other:?}"),
        }

        // MultiDense variant on nearest
        let q = grpc.query.as_ref().expect("query");
        use qdrant::query::Variant as Qv;
        use qdrant::vector_input::Variant as Vi;
        match q.variant.as_ref() {
            Some(Qv::Nearest(vi)) => match vi.variant.as_ref() {
                Some(Vi::MultiDense(md)) => {
                    assert_eq!(md.vectors.len(), 2);
                    assert_eq!(md.vectors[0].data, vec![0.1, 0.2]);
                    assert_eq!(md.vectors[1].data, vec![0.3, 0.4]);
                }
                other => panic!("expected MultiDense, got {other:?}"),
            },
            other => panic!("expected Nearest, got {other:?}"),
        }
    }

    #[test]
    fn rest_grpc_hybrid_prefetch_parity() {
        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();
        let PlannedOperation::Query {
            collection,
            request,
        } = &op
        else {
            panic!("expected Query");
        };

        let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
        assert_eq!(body["query"]["fusion"], "rrf");
        assert_eq!(body["prefetch"].as_array().unwrap().len(), 2);
        assert_eq!(body["prefetch"][0]["using"], "dense");
        assert_eq!(body["prefetch"][1]["using"], "sparse");

        let grpc = test_api::to_query_points(request, collection).unwrap();
        assert_eq!(grpc.prefetch.len(), 2);
        assert_eq!(grpc.prefetch[0].using.as_deref(), Some("dense"));
        assert_eq!(grpc.prefetch[1].using.as_deref(), Some("sparse"));
        use qdrant::query::Variant as Qv;
        match grpc.query.as_ref().and_then(|q| q.variant.as_ref()) {
            Some(Qv::Fusion(f)) => {
                let _ = f;
            }
            other => panic!("expected Fusion query variant, got {other:?}"),
        }
    }

    #[test]
    fn rest_grpc_formula_parity() {
        let stmt =
            Parser::parse("QUERY FORMULA score + 1.0 DEFAULTS (score = 0.0) FROM docs LIMIT 5;")
                .unwrap();
        let op = plan(&stmt).unwrap();
        let PlannedOperation::Query {
            collection,
            request,
        } = &op
        else {
            panic!("expected Query");
        };
        let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
        assert!(
            body["query"].get("formula").is_some(),
            "REST formula missing: {}",
            body["query"]
        );

        let grpc = test_api::to_query_points(request, collection).unwrap();
        use qdrant::query::Variant as Qv;
        match grpc.query.as_ref().and_then(|q| q.variant.as_ref()) {
            Some(Qv::Formula(f)) => {
                assert!(f.expression.is_some(), "gRPC formula must have expression");
            }
            other => panic!("expected Formula, got {other:?}"),
        }
    }

    /// MAX / MIN / ACOSH — new Qdrant Expression variants (proto fields 20-22).
    #[test]
    fn rest_grpc_formula_nary_acosh_parity() {
        let stmt = Parser::parse(
            "QUERY FORMULA MAX(score * 2.0, MIN(score, bonus)) + ACOSH(rank) \
             DEFAULTS (score = 0.0, bonus = 0.0) FROM docs LIMIT 5;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();
        let PlannedOperation::Query {
            collection,
            request,
        } = &op
        else {
            panic!("expected Query");
        };

        // REST: sum → [max: [mult, min: [...] ], acosh]
        let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
        let expr = &body["query"]["formula"];
        let sum = expr["sum"].as_array().expect("sum terms");
        let max = &sum[0]["max"];
        assert!(
            max.is_array() && max.as_array().unwrap().len() == 2,
            "{max}"
        );
        assert!(
            max[1]["min"].as_array().is_some_and(|m| m.len() == 2),
            "nested MIN must lower to a 2-term array: {max}"
        );
        assert_eq!(
            sum[1]["acosh"],
            serde_json::json!("rank"),
            "ACOSH over a bare variable lowers to a string expression: {sum:?}"
        );

        // gRPC: same shape via typed proto variants.
        let grpc = test_api::to_query_points(request, collection).unwrap();
        use qdrant::expression::Variant as Ev;
        use qdrant::query::Variant as Qv;
        let Some(Qv::Formula(formula)) = grpc.query.as_ref().and_then(|q| q.variant.as_ref())
        else {
            panic!("expected Formula query");
        };
        let Some(expression) = formula.expression.as_ref() else {
            panic!("formula missing expression");
        };
        let Some(Ev::Sum(sum)) = expression.variant.as_ref() else {
            panic!("expected Sum expression, got {:?}", expression.variant);
        };
        let Some(Ev::Max(max)) = sum.sum[0].variant.as_ref() else {
            panic!("expected Max expression, got {:?}", sum.sum[0].variant);
        };
        assert_eq!(max.max.len(), 2, "MAX must carry both operands");
        assert!(
            matches!(max.max[1].variant.as_ref(), Some(Ev::Min(_))),
            "nested MIN must map to MinExpression, got {:?}",
            max.max[1].variant
        );
        let Some(Ev::Acosh(_)) = sum.sum[1].variant.as_ref() else {
            panic!("expected Acosh expression, got {:?}", sum.sum[1].variant);
        };
    }

    #[test]
    fn multi_dense_plan_vector_matches_rest_and_grpc_shape() {
        let multi = PlanVectorValue::MultiDense(vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        let rest = serde_json::to_value(&multi).unwrap();
        assert_eq!(
            rest,
            serde_json::json!([[1.0, 2.0], [3.0, 4.0]]),
            "REST multi is array-of-arrays"
        );

        let input = PlanQueryInput::Vector(multi);
        let vi = test_api::to_vector_input(&input);
        use qdrant::vector_input::Variant as Vi;
        match vi.variant {
            Some(Vi::MultiDense(md)) => {
                assert_eq!(md.vectors.len(), 2);
                assert_eq!(md.vectors[0].data, vec![1.0, 2.0]);
            }
            other => panic!("gRPC multi must be MultiDense, got {other:?}"),
        }
    }

    #[test]
    fn cross_rerank_is_not_a_qdrant_query_variant() {
        let stmt = Parser::parse(
            "WITH c AS (QUERY VECTOR [0.1, 0.2] FROM docs USING dense LIMIT 20) \
             QUERY CROSS RERANK TEXT 'q' MODEL 'm' FROM docs PREFETCH (c) LIMIT 5;",
        )
        .unwrap();
        let op = plan(&stmt).unwrap();
        assert!(matches!(op, PlannedOperation::CrossRerank { .. }));
        if let PlannedOperation::Query { request, .. } = &op {
            panic!("must not be Query: {:?}", request.query);
        }
    }

    /// DDL REST body matches OpenAPI CreateCollection / UpdateCollection schemas.
    #[test]
    fn ddl_create_and_update_rest_bodies_match_openapi() {
        let Some(openapi) = openapi_or_skip() else {
            return;
        };

        let create = Parser::parse(
            "CREATE COLLECTION docs (dense VECTOR(384, COSINE) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true), sparse SPARSE) \
             WITH HNSW (m = 16, ef_construct = 100) \
             WITH OPTIMIZERS (indexing_threshold = 20000, max_optimization_threads = 'auto') \
             WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = true, shard_number = 2, sharding_method = 'custom');",
        )
        .unwrap();
        let op = plan(&create).unwrap();
        let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
        assert!(body.get("params").is_none());
        assert!(body.get("shard_keys").is_none());
        assert_eq!(body["replication_factor"], 2);
        assert_eq!(
            body["vectors"]["dense"]["quantization_config"]["scalar"]["type"],
            "int8"
        );
        assert_eq!(
            body["optimizers_config"]["max_optimization_threads"],
            "auto"
        );
        validate_ref(&openapi, "CreateCollection", &body);

        // Per-vector product/binary/turbo nest correctly for OpenAPI VectorParams.
        let create2 = Parser::parse(
            "CREATE COLLECTION docs (v VECTOR(64, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = true) WITH MULTIVECTOR (comparator = 'max_sim'));",
        )
        .unwrap();
        let body2 = to_rest_route(&plan(&create2).unwrap())
            .expect("rest")
            .body_json()
            .unwrap();
        assert_eq!(
            body2["vectors"]["v"]["quantization_config"]["product"]["compression"],
            "x16"
        );
        assert_eq!(
            body2["vectors"]["v"]["multivector_config"]["comparator"],
            "max_sim"
        );
        validate_ref(&openapi, "CreateCollection", &body2);

        let alter = Parser::parse(
            "ALTER COLLECTION docs WITH HNSW (ef_construct = 200) WITH PARAMS (replication_factor = 3) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits');",
        )
        .unwrap();
        let alter_body = to_rest_route(&plan(&alter).unwrap())
            .expect("rest")
            .body_json()
            .unwrap();
        assert_eq!(alter_body["params"]["replication_factor"], 3);
        assert_eq!(
            alter_body["quantization_config"]["binary"]["encoding"],
            "two_bits"
        );
        validate_ref(&openapi, "UpdateCollection", &alter_body);

        let disable =
            Parser::parse("ALTER COLLECTION docs WITH QUANTIZATION (disabled = true);").unwrap();
        let disable_body = to_rest_route(&plan(&disable).unwrap())
            .expect("rest")
            .body_json()
            .unwrap();
        assert_eq!(disable_body["quantization_config"], "Disabled");
        validate_ref(&openapi, "UpdateCollection", &disable_body);
    }

    #[test]
    fn ddl_create_index_rest_nests_field_schema() {
        let Some(openapi) = openapi_or_skip() else {
            return;
        };
        let stmt = Parser::parse(
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true, tokenizer = 'word', min_token_len = 2);",
        )
        .unwrap();
        let body = to_rest_route(&plan(&stmt).unwrap())
            .expect("rest")
            .body_json()
            .unwrap();
        assert_eq!(body["field_name"], "title");
        assert_eq!(body["field_schema"]["type"], "text");
        assert_eq!(body["field_schema"]["lowercase"], true);
        assert_eq!(body["field_schema"]["tokenizer"], "word");
        assert!(body.get("lowercase").is_none());
        validate_ref(&openapi, "CreateFieldIndex", &body);
    }

    /// gRPC create maps the same plan IR fields as REST OpenAPI projection covers.
    #[test]
    fn ddl_create_grpc_reads_same_ir_as_rest_projection() {
        use qql_core::ast::Stmt;
        use qql_plan::ddl::{create_collection_rest_body, lower_create_collection};

        let stmt = Parser::parse(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'scalar', quantile = 0.95, always_ram = true) WITH HNSW (m = 24) WITH MULTIVECTOR (comparator = 'max_sim')) \
             WITH OPTIMIZERS (indexing_threshold = 1000) \
             WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = false);",
        )
        .unwrap();
        let Stmt::CreateCollection(cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(&cc);
        let rest = create_collection_rest_body(&req);
        assert_eq!(rest["replication_factor"], 2);
        assert_eq!(
            rest["vectors"]["v"]["quantization_config"]["scalar"]["quantile"],
            0.95
        );

        // gRPC converters consume flat IR
        let vp = crate::grpc_route::test_api_ddl::vector_params(
            req.vectors.as_ref().unwrap().get("v").unwrap(),
        );
        assert_eq!(vp.size, 128);
        assert!(vp.hnsw_config.is_some());
        assert!(vp.quantization_config.is_some());
        assert!(vp.multivector_config.is_some());
        let hnsw = req.hnsw_config.as_ref().map(|_| ());
        let _ = hnsw;
        assert!(req.optimizers_config.is_some());
    }

    // ── RT-01: Document/Image inference validation ────────────────

    /// Document WITH model serializes as a valid OpenAPI Document object
    /// `{"text": ..., "model": ...}` and passes the OpenAPI VectorInput schema.
    #[test]
    fn document_with_model_is_openapi_valid() {
        let Some(openapi) = openapi_or_skip() else {
            return;
        };
        let stmt = Parser::parse(
            "QUERY TEXT 'hello world' MODEL 'jinaai/jina-embeddings-v2-base-en' FROM docs LIMIT 5;",
        )
        .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        let nearest = &json["query"]["nearest"];
        assert!(nearest.is_object(), "nearest must be an object: {nearest}");
        assert_eq!(nearest["text"], "hello world");
        assert_eq!(nearest["model"], "jinaai/jina-embeddings-v2-base-en");
        validate_ref(&openapi, "Document", nearest);
        validate_ref(&openapi, "Query", &json["query"]);
    }

    /// Image WITH model serializes as a valid OpenAPI Image object
    /// `{"image": ..., "model": ...}` and passes the OpenAPI VectorInput schema.
    #[test]
    fn image_with_model_is_openapi_valid() {
        let Some(openapi) = openapi_or_skip() else {
            return;
        };
        let stmt = Parser::parse(
            "QUERY IMAGE 'https://example.com/photo.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision' FROM docs USING image_vec LIMIT 5;",
        )
        .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        let nearest = &json["query"]["nearest"];
        assert!(nearest.is_object(), "nearest must be an object: {nearest}");
        assert_eq!(nearest["image"], "https://example.com/photo.jpg");
        assert_eq!(nearest["model"], "Qdrant/clip-ViT-B-32-vision");
        // Validate against both Image and VectorInput schemas.
        validate_ref(&openapi, "Image", nearest);
        validate_ref(&openapi, "Query", &json["query"]);
    }

    /// Document WITHOUT model must fail planning with a clear validation error.
    #[test]
    fn document_without_model_plans_successfully() {
        // Plan layer is transport-agnostic — MODEL is filled by the executor.
        let result = plan(&Parser::parse("QUERY 'hello' FROM docs USING dense LIMIT 5;").unwrap());
        assert!(
            result.is_ok(),
            "plan should succeed without MODEL: {}",
            result.unwrap_err()
        );
    }

    /// Image WITHOUT model now plans successfully — MODEL resolution
    /// happens at the executor layer, not in the plan IR.
    #[test]
    fn image_without_model_plans_successfully() {
        let result = plan(
            &Parser::parse(
                "QUERY IMAGE 'https://example.com/photo.jpg' FROM docs USING image_vec LIMIT 5;",
            )
            .unwrap(),
        );
        assert!(
            result.is_ok(),
            "plan should succeed without MODEL: {}",
            result.unwrap_err()
        );
    }

    /// Document with explicit MODEL '' (empty) plans successfully —
    /// the plan IR preserves the value; the executor validates it.
    #[test]
    fn document_with_empty_model_plans_successfully() {
        let result = plan(
            &Parser::parse("QUERY TEXT 'hello' MODEL '' FROM docs USING dense LIMIT 5;").unwrap(),
        );
        assert!(result.is_ok());
    }

    /// HYBRID requires MODEL so both dense and sparse prefetches get a valid
    /// Document object with the model field populated (no bare-string leakage).
    #[test]
    fn hybrid_with_model_propagates_to_both_prefetches() {
        let Some(openapi) = openapi_or_skip() else {
            return;
        };
        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
        let json = try_route(&stmt).unwrap().body_json().unwrap();
        let prefetch = json["prefetch"].as_array().unwrap();
        assert_eq!(prefetch.len(), 2);
        // Both prefetches must be Document objects (not bare strings).
        for (i, pf) in prefetch.iter().enumerate() {
            let nearest = &pf["query"]["nearest"];
            assert!(
                nearest.is_object(),
                "prefetch[{i}] nearest must be Document object, got {nearest}"
            );
            assert_eq!(nearest["model"], "bge");
            validate_ref(&openapi, "Document", nearest);
        }
        validate_ref(&openapi, "QueryRequest", &json);
    }
}
