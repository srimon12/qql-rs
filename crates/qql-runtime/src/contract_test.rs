#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use qql_core::parser::Parser;
    use qql_plan::routing::route;

    fn load_openapi_json() -> Option<serde_json::Value> {
        let paths = ["../../openapi.json", "openapi.json"];
        for p in &paths {
            let path = Path::new(p);
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(json) = serde_json::from_str(&content) {
                        return Some(json);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn test_contract_all_query_variants_match_openapi_json() {
        let openapi = match load_openapi_json() {
            Some(j) => j,
            None => {
                eprintln!("openapi.json not found, skipping contract test");
                return;
            }
        };

        let query_validator = jsonschema::validator_for(&serde_json::json!({
            "$ref": "#/components/schemas/Query",
            "components": openapi["components"]
        }))
        .expect("failed to compile Query schema from openapi.json");

        // Test each query variant by parsing QQL, routing, and validating
        let query_cases: &[(&str, &str)] = &[
            ("sample", "QUERY SAMPLE RANDOM FROM docs LIMIT 10;"),
            ("nearest text", "QUERY 'stroke' FROM docs LIMIT 10;"),
            ("nearest vector", "QUERY NEAREST VECTOR [0.1, 0.2] FROM docs USING dense LIMIT 5;"),
            ("nearest point", "QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 5;"),
            ("recommend", "QUERY RECOMMEND POSITIVE (1) NEGATIVE (2) STRATEGY average_vector FROM docs USING dense LIMIT 10;"),
            ("context", "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs LIMIT 10;"),
            ("discover", "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;"),
            ("order_by", "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;"),
            ("fusion", "WITH a AS (QUERY 'x' FROM docs USING dense LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 10;"),
            ("mmr", "QUERY MMR TEXT 'x' DIVERSITY 0.4 CANDIDATES 100 FROM docs USING dense LIMIT 5;"),
            ("rerank", "QUERY RERANK TEXT 'travel' MODEL 'colbert' FROM docs USING colbert PREFETCH (QUERY 'travel' FROM docs USING dense LIMIT 50) LIMIT 10;"),
        ];

        for (name, qql) in query_cases {
            let stmt =
                Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {}: {e}", name));
            let r = route(&stmt);
            let json = r
                .body_json()
                .unwrap_or_else(|| panic!("no body for {}", name));
            let query = json
                .get("query")
                .unwrap_or_else(|| panic!("no query field in body for {}: {json}", name));

            let errors: Vec<_> = query_validator.iter_errors(query).collect();
            assert!(
                errors.is_empty(),
                "Contract Violation: {} query failed openapi.json schema validation: {:?}\nQuery JSON: {query}",
                name, errors
            );
        }

        // Test Filter schema against all filter variants
        let filter_validator = jsonschema::validator_for(&serde_json::json!({
            "$ref": "#/components/schemas/Filter",
            "components": openapi["components"]
        }))
        .expect("failed to compile Filter schema from openapi.json");

        let filter_cases: &[(&str, &str)] = &[
            ("equality", "QUERY 'x' FROM docs WHERE status = 'active';"),
            ("inequality range", "QUERY 'x' FROM docs WHERE age >= 21 AND score < 100.0;"),
            ("between", "QUERY 'x' FROM docs WHERE age BETWEEN 20 AND 30;"),
            ("in list", "QUERY 'x' FROM docs WHERE tag IN ('a', 'b', 'c');"),
            ("is null", "QUERY 'x' FROM docs WHERE deleted_at IS NULL;"),
            ("is empty", "QUERY 'x' FROM docs WHERE tags IS EMPTY;"),
            ("match text", "QUERY 'x' FROM docs WHERE body MATCH 'hello world';"),
            ("match phrase", "QUERY 'x' FROM docs WHERE body MATCH PHRASE 'hello world';"),
            ("match any", "QUERY 'x' FROM docs WHERE body MATCH ANY ('hello', 'world');"),
            ("has vector", "QUERY 'x' FROM docs WHERE HAS_VECTOR 'dense';"),
            ("values count", "QUERY 'x' FROM docs WHERE tags VALUES_COUNT >= 2;"),
            ("nested", "QUERY 'x' FROM docs WHERE NESTED('reviews', rating > 4);"),
            ("geo bbox", "QUERY 'x' FROM docs WHERE location GEO_BBOX { top_left: {lat: 52.52, lon: 13.40}, bottom_right: {lat: 52.51, lon: 13.41} };"),
            ("geo radius", "QUERY 'x' FROM docs WHERE location GEO_RADIUS { center: {lat: 52.52, lon: 13.40}, radius: 1000.0 };"),
            ("point id eq", "QUERY 'x' FROM docs WHERE id = 42;"),
            ("point id in", "QUERY 'x' FROM docs WHERE id IN (1, 2, 3);"),
            ("compound or not", "QUERY 'x' FROM docs WHERE (status = 'a' OR status = 'b') AND NOT category = 'c';"),
        ];

        for (name, qql) in filter_cases {
            let stmt =
                Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {}: {e}", name));
            let r = route(&stmt);
            let json = r
                .body_json()
                .unwrap_or_else(|| panic!("no body for {}", name));
            let filter = json
                .get("filter")
                .unwrap_or_else(|| panic!("no filter field in body for {}: {json}", name));

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
                "Contract Violation: {} filter failed openapi.json schema validation: {:?}\nFilter JSON: {norm_filter}",
                name, errors
            );
        }

        // Test ScrollRequest schema
        let scroll_validator = jsonschema::validator_for(&serde_json::json!({
            "$ref": "#/components/schemas/ScrollRequest",
            "components": openapi["components"]
        }))
        .expect("failed to compile ScrollRequest schema from openapi.json");

        let scroll_stmt =
            Parser::parse("SCROLL FROM docs WHERE status = 'active' LIMIT 50;").unwrap();
        let scroll_route = route(&scroll_stmt);
        let scroll_json = scroll_route.body_json().unwrap();
        let scroll_errors: Vec<_> = scroll_validator.iter_errors(&scroll_json).collect();
        assert!(
            scroll_errors.is_empty(),
            "Contract Violation: ScrollRequest failed openapi.json schema validation: {:?}\nJSON: {scroll_json}",
            scroll_errors
        );

        // Test PointRequest schema
        let points_validator = jsonschema::validator_for(&serde_json::json!({
            "$ref": "#/components/schemas/PointRequest",
            "components": openapi["components"]
        }))
        .expect("failed to compile PointRequest schema from openapi.json");

        let points_stmt =
            Parser::parse("QUERY POINTS (42, 'uuid-v4') FROM docs WITH PAYLOAD INCLUDE ('title');")
                .unwrap();
        let points_route = route(&points_stmt);
        let points_json = points_route.body_json().unwrap();
        let points_errors: Vec<_> = points_validator.iter_errors(&points_json).collect();
        assert!(
            points_errors.is_empty(),
            "Contract Violation: PointRequest failed openapi.json schema validation: {:?}\nJSON: {points_json}",
            points_errors
        );
    }
}
