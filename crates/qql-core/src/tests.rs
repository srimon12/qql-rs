use crate::ast::{
    ComparisonOp, FilterExpr, FusionMethod, PointId, PointIdPredicate, PointSelector,
    QueryCollection, QueryExpr, QueryInput, Stmt, Value, VectorValue,
};
use crate::error::{ErrorKind, QqlError, Span};
use crate::explain;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn query(source: &str) -> Box<crate::ast::QueryStmt> {
    match Parser::parse(source).expect("query must parse") {
        Stmt::Query(query) => query,
        statement => panic!("expected query, got {statement:?}"),
    }
}

#[test]
fn direct_points_and_nearest_point_are_distinct() {
    let direct = query("QUERY POINTS (42, 'point-a') FROM docs WITH PAYLOAD true;");
    assert!(matches!(
        direct.expression,
        QueryExpr::Points { ref ids }
            if ids == &[
                PointId::Number(42),
                PointId::String("point-a".into())
            ]
    ));

    let nearest = query("QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 5;");
    assert!(matches!(
        nearest.expression,
        QueryExpr::Nearest {
            input: QueryInput::Point(PointId::Number(42)),
            using: Some(ref using),
            ..
        } if using == "dense"
    ));
    assert!(Parser::parse("SELECT * FROM docs WHERE id = 42").is_err());
}

#[test]
fn nearest_inputs_are_explicit_and_text_has_a_short_form() {
    let text = query("QUERY 'hello' FROM docs;");
    assert!(matches!(
        text.expression,
        QueryExpr::Nearest {
            input: QueryInput::Text { ref text, model: None },
            ..
        } if text == "hello"
    ));

    let vector = query("QUERY NEAREST VECTOR [0.1, 0.2] FROM docs;");
    assert!(matches!(
        vector.expression,
        QueryExpr::Nearest {
            input: QueryInput::Vector(VectorValue::Dense(ref values)),
            ..
        } if values == &[0.1, 0.2]
    ));
    assert!(Parser::parse("QUERY 42 FROM docs").is_err());
}

#[test]
fn universal_query_variants_are_closed_typed_states() {
    let cases = [
        "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY average_vector FROM docs;",
        "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs;",
        "QUERY DISCOVER TARGET POINT 1 CONTEXT (POSITIVE POINT 2 NEGATIVE POINT 3) FROM docs;",
        "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
        "QUERY SAMPLE RANDOM FROM docs LIMIT 10;",
        "QUERY FORMULA $score + 1 DEFAULTS (missing = 0) FROM docs;",
        "QUERY RELEVANCE FEEDBACK TARGET POINT 1 FEEDBACK ((POINT 2, 0.8)) STRATEGY naive (a = 1, b = 0.5, c = 0.25) FROM docs;",
        "QUERY MMR TEXT 'diverse' DIVERSITY 0.4 CANDIDATES 50 FROM docs USING dense;",
        "QUERY HYBRID TEXT 'hybrid' DENSE dense SPARSE sparse FUSION DBSF FROM docs;",
    ];
    for source in cases {
        query(source);
    }

    assert!(matches!(
        query(cases[8]).expression,
        QueryExpr::Hybrid {
            fusion: FusionMethod::Dbsf,
            ..
        }
    ));
}

#[test]
fn fusion_and_rerank_require_complete_prefetch_topology() {
    let fusion = query(
        "WITH dense AS (QUERY TEXT 'x' USING dense LIMIT 100), sparse AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (dense, sparse) LIMIT 10;",
    );
    assert_eq!(fusion.ctes.len(), 2);
    assert!(fusion
        .ctes
        .iter()
        .all(|cte| cte.query.collection == QueryCollection::Inherited));
    assert!(matches!(
        fusion.expression,
        QueryExpr::Fusion {
            method: FusionMethod::Rrf,
            ref prefetch,
        } if prefetch.len() == 2
    ));

    let rerank = query(
        "WITH candidates AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY RERANK TEXT 'x' MODEL 'reranker' FROM docs USING colbert PREFETCH (candidates) LIMIT 10;",
    );
    assert!(matches!(
        rerank.expression,
        QueryExpr::Rerank {
            ref model,
            ref using,
            ref prefetch,
            ..
        } if model == "reranker" && using == "colbert" && prefetch.len() == 1
    ));

    for source in [
        "QUERY FUSION RRF FROM docs;",
        "QUERY FUSION RRF FROM docs PREFETCH (missing);",
        "QUERY RERANK TEXT 'x' MODEL 'm' FROM docs PREFETCH (candidates);",
        "QUERY RERANK TEXT 'x' MODEL 'm' FROM docs USING colbert;",
    ] {
        assert_eq!(
            Parser::parse(source)
                .expect_err("incomplete topology must fail")
                .kind,
            ErrorKind::Validation
        );
    }
}

#[test]
fn query_clauses_have_one_order_and_are_not_permissive() {
    query(
        "QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2;",
    );

    for source in [
        "QUERY TEXT 'x' FROM docs LIMIT 10 WHERE active = true;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 LIMIT 20;",
        "QUERY TEXT 'x' FROM docs USING;",
        "QUERY TEXT 'x' FROM docs SCORE THRESHOLD;",
        "QUERY TEXT 'x' USING dense;",
        "QUERY TEXT 'x' FROM docs WITH (exact = true);",
        "QUERY MMR TEXT 'x' DIVERSITY 0.5 FROM docs;",
    ] {
        assert!(Parser::parse(source).is_err(), "must reject: {source}");
    }
}

#[test]
fn parse_all_requires_semicolon_separators_without_empty_statements() {
    assert_eq!(
        Parser::parse_all("SHOW COLLECTIONS; SHOW COLLECTION docs;")
            .expect("script must parse")
            .len(),
        2
    );
    for source in [
        "SHOW COLLECTIONS SHOW COLLECTION docs",
        "; SHOW COLLECTIONS",
        "SHOW COLLECTIONS;; SHOW COLLECTION docs",
    ] {
        assert!(Parser::parse_all(source).is_err(), "must reject: {source}");
    }
    assert!(Parser::parse("SHOW COLLECTIONS;;").is_err());
}

#[test]
fn errors_have_explicit_kinds_codes_and_spans() {
    let parse = Parser::parse("SELECT").expect_err("SELECT is not a statement");
    assert_eq!(parse.kind, ErrorKind::Parse);
    assert_eq!(parse.span, Some(Span::new(0, 6)));
    assert!(!parse.code.is_empty());

    let lex = Parser::parse("@").expect_err("invalid character must fail lexing");
    assert_eq!(lex.kind, ErrorKind::Lex);
    assert_eq!(lex.span, Some(Span::new(0, 1)));

    let token = Lexer::new("QUERY").next().unwrap().unwrap();
    assert_eq!(token.span, Span::new(0, 5));

    // Runtime error kinds
    let exec = QqlError::execution("TEST", "execution error", None);
    assert_eq!(exec.kind, ErrorKind::Execution);

    let transport = QqlError::transport("TEST", "transport error", None);
    assert_eq!(transport.kind, ErrorKind::Transport);

    let backend = QqlError::backend("TEST", "backend error", None);
    assert_eq!(backend.kind, ErrorKind::Backend);
}

#[test]
fn filters_are_typed_and_negative_forms_are_normalized() {
    let statement = query(
        "QUERY TEXT 'x' FROM docs WHERE id IN (1, 'a') AND status != 'deleted' AND tags MATCH ANY ('rust', 'qdrant');",
    );
    let FilterExpr::And { operands } = *statement.filter.unwrap() else {
        panic!("expected conjunction");
    };
    assert!(matches!(
        operands[0],
        FilterExpr::PointId(PointIdPredicate::In(_))
    ));
    assert!(matches!(operands[1], FilterExpr::Not { .. }));
    assert!(matches!(
        operands[2],
        FilterExpr::MatchAny { ref values, .. } if values.len() == 2
    ));
    assert!(Parser::parse("QUERY TEXT 'x' FROM docs WHERE id > 4").is_err());
    assert!(Parser::parse("QUERY TEXT 'x' FROM docs WHERE tags MATCH ANY 'x y'").is_err());
}

#[test]
fn geo_filters_validate_required_fields_and_ranges() {
    query(
        "QUERY TEXT 'x' FROM docs WHERE location GEO_RADIUS {center: {lat: 52.5, lon: 13.4}, radius: 1000};",
    );
    for source in [
        "QUERY TEXT 'x' FROM docs WHERE location GEO_RADIUS {center: {lat: 91, lon: 13}, radius: 1};",
        "QUERY TEXT 'x' FROM docs WHERE location GEO_RADIUS {center: {lat: 1}, radius: 1};",
        "QUERY TEXT 'x' FROM docs WHERE location GEO_RADIUS {center: {lat: 1, lon: 2}, radius: 0};",
    ] {
        assert_eq!(
            Parser::parse(source).expect_err("invalid geo must fail").kind,
            ErrorKind::Validation
        );
    }
}

#[test]
fn duplicate_payload_and_config_keys_are_case_insensitive() {
    for source in [
        "UPSERT INTO docs VALUES {id: 1, Title: 'a', title: 'b'};",
        "CREATE COLLECTION docs WITH HNSW (m = 8, M = 16);",
        "CREATE INDEX ON COLLECTION docs FOR title WITH (on_disk = true, ON_DISK = false);",
    ] {
        let error = Parser::parse(source).expect_err("duplicate key must fail");
        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.code, "QQL-PARSE-DUPLICATE-KEY");
    }
}

#[test]
fn upsert_and_updates_have_typed_ids_vectors_and_selectors() {
    let upsert = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'dense', vector: [0.1, 0.2]}, {id: 'p2', meta: {nested: [1, true, null]}, vector: {sparse: {indices: [1, 4], values: [0.5, 0.8]}, multi: [[0.1, 0.2], [0.3, 0.4]]}};",
    )
    .expect("upsert must parse");
    let Stmt::Upsert(upsert) = upsert else {
        panic!("expected upsert");
    };
    assert_eq!(upsert.points.len(), 2);
    assert_eq!(upsert.points[0].id, PointId::Number(1));
    assert_eq!(upsert.points[1].payload[0].0, "meta");

    let update = Parser::parse(
        "UPDATE docs SET VECTOR sparse = {indices: [1, 3], values: [0.4, 0.9]} WHERE id = 'p2';",
    )
    .expect("vector update must parse");
    assert!(matches!(
        update,
        Stmt::UpdateVector(ref update)
            if matches!(update.vector, VectorValue::Sparse { .. })
    ));

    let delete = Parser::parse("DELETE FROM docs WHERE id IN (1, 2);").expect("delete must parse");
    assert!(matches!(
        delete,
        Stmt::Delete(ref delete) if matches!(delete.selector, PointSelector::Ids(_))
    ));
}

#[test]
fn ddl_and_scroll_remain_available() {
    for source in [
        "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE) WITH HNSW (m = 16);",
        "ALTER COLLECTION docs WITH VECTOR (on_disk = true);",
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);",
        "SCROLL FROM docs WHERE active = true AFTER 10 LIMIT 20;",
        "DROP COLLECTION docs;",
        "SHOW COLLECTIONS;",
    ] {
        Parser::parse(source).unwrap_or_else(|error| panic!("failed to parse {source}: {error}"));
    }
}

#[test]
fn explain_reports_ast_intent_without_runtime_claims() {
    let output = explain::explain("QUERY NEAREST POINT 1 FROM docs;").unwrap();
    assert!(output.contains("nearest neighbors from a point"));
    assert!(!output.contains("ColBERT"));
    assert!(!output.contains("server"));
}

#[test]
fn filter_injection_uses_typed_operators() {
    let mut statement = Parser::parse("QUERY TEXT 'x' FROM docs WHERE active = true;").unwrap();
    crate::ast::inject_filter(
        &mut statement,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(query) = statement else {
        panic!("expected query");
    };
    assert!(matches!(*query.filter.unwrap(), FilterExpr::And { .. }));
}

#[cfg(feature = "json")]
#[test]
fn json_conversion_is_fallible_for_non_finite_floats() {
    assert!(Value::Float(f64::NAN).to_json().is_err());
    assert_eq!(
        Value::from_json(serde_json::json!({"nested": [1, true, null]})).unwrap(),
        Value::Dict(vec![(
            "nested".into(),
            Value::List(vec![Value::Int(1), Value::Bool(true), Value::Null])
        )])
    );
}
