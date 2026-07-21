#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use qql_core::parser::Parser;
    use qql_plan::routing::route;

    #[test]
    fn test_contract_all_query_variants_match_openapi_json() {
        let path = Path::new("../../openapi.json");
        if !path.exists() {
            eprintln!("openapi.json not found at ../../openapi.json, skipping contract test");
            return;
        }

        let content = fs::read_to_string(path).expect("failed to read openapi.json");
        let openapi: serde_json::Value =
            serde_json::from_str(&content).expect("invalid openapi.json");

        let validator = jsonschema::validator_for(&serde_json::json!({
            "$ref": "#/components/schemas/Query",
            "components": openapi["components"]
        }))
        .expect("failed to compile Query schema from openapi.json");

        // Test each query variant by parsing QQL, routing, and validating
        let cases: &[(&str, &str)] = &[
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

        for (name, qql) in cases {
            let stmt =
                Parser::parse(qql).unwrap_or_else(|e| panic!("parse failed for {}: {e}", name));
            let r = route(&stmt);
            let json = r
                .body_json()
                .unwrap_or_else(|| panic!("no body for {}", name));
            let query = json
                .get("query")
                .unwrap_or_else(|| panic!("no query field in body for {}: {json}", name));

            let errors: Vec<_> = validator.iter_errors(query).collect();
            assert!(
                errors.is_empty(),
                "Contract Violation: {} query failed openapi.json schema validation: {:?}\nQuery JSON: {query}",
                name, errors
            );
        }
    }
}
