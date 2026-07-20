#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    use crate::pipeline::{
        QueryPointsRequest, QueryVariant, WithPayload,
    };
    use crate::rest::query_request_json;

    #[test]
    fn test_contract_all_query_variants_match_openapi_json() {
        let path = Path::new("../../openapi.json");
        if !path.exists() {
            eprintln!("openapi.json not found at ../../openapi.json, skipping contract test");
            return;
        }

        let content = fs::read_to_string(path).expect("failed to read openapi.json");
        let openapi: serde_json::Value = serde_json::from_str(&content).expect("invalid openapi.json");

        // Validate sample request JSON structure
        let req_sample = QueryPointsRequest {
            collection_name: "test_coll".to_string(),
            query: Some(QueryVariant::Sample),
            prefetch: Vec::new(),
            limit: 10,
            offset: 0,
            params: None,
            filter: None,
            with_payload: Some(WithPayload {
                enable: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
            }),
            with_vectors: None,
            score_threshold: None,
            lookup_from: None,
            using: None,
            timeout: None,
        };

        let json_sample = query_request_json(&req_sample).expect("failed to serialize Sample");
        assert_eq!(
            json_sample.get("query"),
            Some(&serde_json::json!({ "sample": "random" }))
        );

        // Validate formula request JSON structure
        let mut defaults = HashMap::new();
        defaults.insert("score".to_string(), 1.0);
        let req_formula = QueryPointsRequest {
            collection_name: "test_coll".to_string(),
            query: Some(QueryVariant::Formula {
                expression: serde_json::json!({
                    "mult": ["$score", 0.3]
                }),
                defaults,
            }),
            prefetch: Vec::new(),
            limit: 10,
            offset: 0,
            params: None,
            filter: None,
            with_payload: None,
            with_vectors: None,
            score_threshold: None,
            lookup_from: None,
            using: None,
            timeout: None,
        };

        let json_formula = query_request_json(&req_formula).expect("failed to serialize Formula");
        let query_val = json_formula.get("query").unwrap();
        assert_eq!(
            query_val,
            &serde_json::json!({
                "formula": { "mult": ["$score", 0.3] }
            })
        );

        // Validate document query request JSON structure
        let req_doc = QueryPointsRequest {
            collection_name: "test_coll".to_string(),
            query: Some(QueryVariant::Document {
                text: "stroke".to_string(),
                model: String::new(),
                options: HashMap::new(),
            }),
            prefetch: Vec::new(),
            limit: 10,
            offset: 0,
            params: None,
            filter: None,
            with_payload: None,
            with_vectors: None,
            score_threshold: None,
            lookup_from: None,
            using: None,
            timeout: None,
        };

        let json_doc = query_request_json(&req_doc).expect("failed to serialize Document");
        assert_eq!(
            json_doc.get("query"),
            Some(&serde_json::json!({ "nearest": "stroke" }))
        );

        let expr = qql_core::ast::FormulaExpr::Case {
            cond: Box::new(qql_core::ast::FilterExpr::Compare {
                field: "priority",
                op: "=",
                value: qql_core::ast::Value::Str(std::borrow::Cow::Borrowed("high")),
            }),
            then_: Box::new(qql_core::ast::FormulaExpr::Constant { value: 2.0 }),
            else_: Box::new(qql_core::ast::FormulaExpr::Constant { value: 1.0 }),
        };
        let formula_json = crate::pipeline::formula_nodes::build_expression(&expr).expect("failed to build CASE expression");
        let req_case = QueryPointsRequest {
            collection_name: "test_coll".to_string(),
            query: Some(QueryVariant::Formula {
                expression: formula_json,
                defaults: HashMap::new(),
            }),
            limit: 5,
            offset: 0,
            params: None,
            prefetch: Vec::new(),
            filter: None,
            with_payload: None,
            with_vectors: None,
            score_threshold: None,
            lookup_from: None,
            using: None,
            timeout: None,
        };
        let json_case = query_request_json(&req_case).expect("failed to serialize CASE WHEN");

        // Verify JSON Schema validity against openapi.json Query schema
        let query_schema = serde_json::json!({
            "$ref": "#/components/schemas/Query",
            "components": openapi["components"]
        });

        let validator = jsonschema::validator_for(&query_schema)
            .expect("failed to compile Query schema from openapi.json");

        for (name, req) in [
            ("sample", json_sample.get("query").unwrap()),
            ("formula", json_formula.get("query").unwrap()),
            ("doc", json_doc.get("query").unwrap()),
            ("case_when", json_case.get("query").unwrap()),
        ] {
            let errors: Vec<_> = validator.iter_errors(req).collect();
            assert!(
                errors.is_empty(),
                "Contract Violation: {} query failed openapi.json schema validation: {:?}",
                name,
                errors
            );
        }
    }
}
