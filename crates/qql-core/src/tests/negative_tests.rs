use crate::error::ErrorKind;
use crate::parser::Parser;

macro_rules! assert_parse_err {
    ($source:expr, $kind:ident) => {{
        let err = Parser::parse($source).expect_err(&format!("expected error for: {}", $source));
        assert_eq!(
            err.kind,
            ErrorKind::Parse,
            "wrong error kind for: {}",
            $source
        );
    }};
}

macro_rules! assert_validation_err {
    ($source:expr) => {{
        let err = Parser::parse($source).expect_err(&format!("expected error for: {}", $source));
        assert_eq!(
            err.kind,
            ErrorKind::Validation,
            "wrong error kind for: {}",
            $source
        );
    }};
}

#[test]
fn top_level_query_requires_from() {
    assert!(Parser::parse("QUERY USING dense;").is_err());
}

#[test]
fn scripts_have_a_bounded_statement_count() {
    let script = std::iter::repeat_n("COUNT FROM docs", crate::parser::MAX_STATEMENTS + 1)
        .collect::<Vec<_>>()
        .join(";");
    let error = Parser::parse_all(&script).expect_err("statement limit must be enforced");
    assert_eq!(error.code, "QQL-PARSE-STATEMENT-LIMIT");
}

#[test]
fn clause_ordering_violations() {
    let invalid = [
        "QUERY TEXT 'x' FROM docs LIMIT 10 WHERE active = true;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 LIMIT 20;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 OFFSET 5 LIMIT 5;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 WHERE x = 1 LIMIT 5;",
    ];
    for source in invalid {
        let err = Parser::parse(source)
            .expect_err(&format!("expected clause order error for: {}", source));
        assert_eq!(err.kind, ErrorKind::Parse);
    }
}

#[test]
fn bare_using_rejected() {
    assert_parse_err!("QUERY TEXT 'x' FROM docs USING;", Parse);
}

#[test]
fn formula_max_min_require_at_least_one_operand() {
    // Qdrant MaxExpression / MinExpression require ≥ 1 operand.
    for source in [
        "QUERY FORMULA MAX() DEFAULTS (score = 0.0) FROM docs;",
        "QUERY FORMULA MIN() DEFAULTS (score = 0.0) FROM docs;",
    ] {
        let err = Parser::parse(source).expect_err("expected empty-operand error");
        assert_eq!(
            err.kind,
            ErrorKind::Parse,
            "wrong error kind for: {}",
            source
        );
        assert_eq!(err.code, "QQL-PARSE-SYNTAX", "wrong code for: {}", source);
    }
}

#[test]
fn bare_score_threshold_rejected() {
    assert_parse_err!("QUERY TEXT 'x' FROM docs SCORE THRESHOLD;", Parse);
}

#[test]
fn top_level_query_without_from_rejected() {
    assert_validation_err!("QUERY TEXT 'x' USING dense;");
}

#[test]
fn generic_with_clause_rejected() {
    assert_parse_err!("QUERY TEXT 'x' FROM docs WITH (exact = true);", Parse);
}

#[test]
fn incomplete_mmr_rejected() {
    assert!(Parser::parse("QUERY MMR TEXT 'x' DIVERSITY 0.5 FROM docs;").is_err());
}

#[test]
fn fusion_without_prefetch_rejected() {
    assert_validation_err!("QUERY FUSION RRF FROM docs;");
}

#[test]
fn fusion_with_missing_prefetch_rejected() {
    assert_validation_err!("QUERY FUSION RRF FROM docs PREFETCH (missing);");
}

#[test]
fn rerank_without_prefetch_rejected() {
    let err = Parser::parse("QUERY RERANK TEXT 'x' MODEL 'm' FROM docs USING colbert;")
        .expect_err("should fail");
    assert_eq!(err.kind, ErrorKind::Validation);
}

#[test]
fn duplicate_keys_rejected() {
    let cases = [
        "UPSERT INTO docs VALUES {id: 1, Title: 'a', title: 'b'};",
        "CREATE COLLECTION docs WITH HNSW (m = 8, M = 16);",
        "CREATE INDEX ON COLLECTION docs FOR title WITH (on_disk = true, ON_DISK = false);",
    ];
    for source in cases {
        let err = Parser::parse(source)
            .expect_err(&format!("expected duplicate key error for: {}", source));
        assert_eq!(err.kind, ErrorKind::Parse);
    }
}

#[test]
fn invalid_geo_rejected() {
    let cases = [
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_RADIUS {center: {lat: 91, lon: 13}, radius: 1};",
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_RADIUS {center: {lat: 1}, radius: 1};",
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_RADIUS {center: {lat: 1, lon: 2}, radius: 0};",
    ];
    for source in cases {
        let err = Parser::parse(source).expect_err(&format!("expected geo error for: {}", source));
        assert_eq!(err.kind, ErrorKind::Validation);
    }
}

#[test]
fn invalid_geo_polygon_rejected() {
    let cases: &[&str] = &[
        // exterior has fewer than 3 points
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_POLYGON {exterior: [{lat: 1, lon: 2}, {lat: 3, lon: 4}]};",
        // exterior has invalid lat
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_POLYGON {exterior: [{lat: 91, lon: 0}, {lat: 0, lon: 0}, {lat: 0, lon: 0}]};",
        // interior ring has fewer than 3 points
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_POLYGON {exterior: [{lat: 0, lon: 0}, {lat: 1, lon: 0}, {lat: 0, lon: 1}], interiors: [[{lat: 0, lon: 0}, {lat: 1, lon: 0}]]};",
        // missing exterior key
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_POLYGON {};",
        // exterior is not a list
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_POLYGON {exterior: {lat: 0, lon: 0}};",
    ];
    for source in cases {
        let err = Parser::parse(source)
            .expect_err(&format!("expected geo polygon error for: {}", source));
        assert_eq!(
            err.kind,
            ErrorKind::Validation,
            "unexpected error kind for: {source}"
        );
    }
}

#[test]
fn id_predicate_inequality_rejected() {
    assert!(Parser::parse("QUERY TEXT 'x' FROM docs WHERE id > 4").is_err());
}

#[test]
fn empty_in_list_rejected() {
    assert_parse_err!("QUERY POINTS (1) FROM docs WHERE tag IN ();", Parse);
    assert_parse_err!("QUERY POINTS (1) FROM docs WHERE tag NOT IN ();", Parse);
}

#[test]
fn invalid_shard_params_rejected() {
    assert_validation_err!(
        "CREATE COLLECTION docs (dense VECTOR (4, Cosine)) WITH PARAMS (sharding_method = true);"
    );
    assert_validation_err!(
        "CREATE COLLECTION docs (dense VECTOR (4, Cosine)) WITH PARAMS (shard_keys = [\"a\", true]);"
    );
    assert_validation_err!(
        "CREATE COLLECTION docs (dense VECTOR (4, Cosine)) WITH PARAMS (shard_number = true);"
    );
}

#[test]
fn match_any_requires_list() {
    assert!(Parser::parse("QUERY TEXT 'x' FROM docs WHERE tags MATCH ANY 'x y'").is_err());
}

#[test]
fn empty_prefetch_rejected() {
    let err = Parser::parse("WITH d AS (QUERY TEXT 'x' USING d LIMIT 10) QUERY FUSION RRF FROM docs PREFETCH () LIMIT 10;")
        .expect_err("empty prefetch should fail");
    assert_eq!(err.kind, ErrorKind::Parse);
}

#[test]
fn unknown_cte_rejected() {
    let err = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (nonexistent) LIMIT 10;",
    ).expect_err("unknown CTE should fail");
    assert_eq!(err.kind, ErrorKind::Validation);
}

#[test]
fn duplicate_cte_name_rejected() {
    let err = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' LIMIT 10), d AS (QUERY TEXT 'y' LIMIT 10) QUERY TEXT 'z' FROM docs;",
    ).expect_err("duplicate CTE should fail");
    assert_eq!(err.kind, ErrorKind::Parse);
}

#[test]
fn semicolon_script_separation() {
    assert!(Parser::parse_all("; SHOW COLLECTIONS").is_err());
    assert!(Parser::parse_all("SHOW COLLECTIONS;; SHOW COLLECTION docs").is_err());
}

#[test]
fn trailing_tokens_rejected() {
    assert!(Parser::parse("SHOW COLLECTIONS extra;").is_err());
    assert!(Parser::parse("SHOW COLLECTIONS FROM").is_err());
}

#[test]
fn non_finite_float_literals_rejected() {
    // grammar.pest numbers are finite; exponent overflow must not yield inf.
    let cases = [
        "UPSERT INTO docs VALUES {id: 1, v: 1e999};",
        "QUERY TEXT 'x' FROM docs SCORE THRESHOLD 1e999 LIMIT 5;",
        "QUERY FORMULA 1e999 FROM docs LIMIT 5;",
        "QUERY FORMULA -1e999 FROM docs LIMIT 5;",
        "QUERY TEXT 'x' FROM docs WHERE score >= -1e999 LIMIT 5;",
        "QUERY [1e999, 0.2] FROM docs;",
        "QUERY VECTOR [-1e999] FROM docs;",
        "UPSERT INTO docs VALUES {id: 1, vector: [1e999]};",
        "UPSERT INTO docs VALUES {id: 1, vector: {indices: [1], values: [1e999]}};",
        "QUERY 'x' FROM docs WITH (mmr = true, diversity = 1e999);",
        "QUERY 'x' FROM docs PARAMS (oversampling = 1e999);",
    ];
    for source in cases {
        let err = Parser::parse(source).expect_err(&format!(
            "expected non-finite float rejection for: {source}"
        ));
        assert!(
            matches!(err.kind, ErrorKind::Parse | ErrorKind::Validation),
            "unexpected error kind for: {source} (kind {:?}, code {})",
            err.kind,
            err.code
        );
    }
}

#[test]
fn limit_beyond_u64_and_zero_rejected_with_positive_integer_code() {
    // Integer literals larger than u64::MAX must be rejected at parse time,
    // not silently clamped or wrapped; LIMIT/OFFSET are `positive_integer`
    // productions carried as u64.
    //
    // LIMIT 0 is rejected too — live-verified against Qdrant 1.19.1: its
    // /points/query API answers 422 "internal.limit: value 0 invalid, must
    // be 1 or larger", so a parse-time rejection beats a runtime 422. (The
    // one-shot acceptance was reverted on that evidence.)
    let cases = [
        "QUERY VECTOR [0.1] FROM docs USING dense LIMIT 18446744073709551616;",
        "SCROLL FROM docs LIMIT 18446744073709551616;",
        "QUERY VECTOR [0.1] FROM docs USING dense LIMIT -1;",
        "QUERY VECTOR [0.1] FROM docs USING dense LIMIT 0;",
        "SCROLL FROM docs LIMIT 0;",
    ];
    for source in cases {
        let err =
            Parser::parse(source).expect_err(&format!("expected limit rejection for: {source}"));
        assert_eq!(
            err.code, "QQL-PARSE-POSITIVE-INTEGER",
            "unexpected code for: {source}"
        );
    }
}

#[test]
fn count_clause_order_enforced() {
    // grammar `count` order is WHERE → SHARD → WITH, each at most once.
    let cases = [
        "COUNT FROM docs WITH (exact = true) SHARD 'x';",
        "COUNT FROM docs SHARD 'a' SHARD 'b';",
        "COUNT FROM docs WITH (exact = true) WITH (exact = false);",
        "COUNT FROM docs WHERE active = true SHARD 'x' WITH (exact = true) SHARD 'y';",
    ];
    for source in cases {
        let err = Parser::parse(source)
            .expect_err(&format!("expected count clause order error for: {source}"));
        assert_eq!(err.code, "QQL-PARSE-CLAUSE-ORDER");
    }
}

#[test]
fn count_valid_grammar_order_accepted() {
    for source in [
        "COUNT FROM docs;",
        "COUNT FROM docs WHERE active = true;",
        "COUNT FROM docs SHARD 'x';",
        "COUNT FROM docs WHERE active = true SHARD 'x' WITH (exact = true);",
        "COUNT FROM docs WITH (exact = false);",
    ] {
        Parser::parse(source).unwrap_or_else(|e| panic!("{source} should parse: {e}"));
    }
}

#[test]
fn count_config_rejects_unknown_keys_and_non_boolean_exact() {
    // F-14: `COUNT ... WITH (...)` previously swallowed bad values silently.
    // Only `exact = <boolean>` is valid; unknown keys and non-boolean values
    // are rejected with a structured code.
    let cases = [
        "COUNT FROM docs WITH (exact = 5);",
        "COUNT FROM docs WITH (exact = 'yes');",
        "COUNT FROM docs WITH (exact = true, foo = 1);",
        "COUNT FROM docs WITH (foo = 1);",
        "COUNT FROM docs WHERE active = true WITH (exact = 1.0);",
    ];
    for source in cases {
        let err =
            Parser::parse(source).expect_err(&format!("expected count config error for: {source}"));
        assert_eq!(err.code, "QQL-PARSE-COUNT-CONFIG");
    }
}

#[test]
fn create_shard_key_config_rejects_unknown_keys_and_invalid_values() {
    // F-15: `CREATE SHARD KEY ... WITH (...)` previously skipped invalid
    // values and ignored unknown keys. Only positive-integer
    // `shards_number` / `replication_factor` are valid.
    let cases = [
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 0);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = -1);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2.0);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2.5);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 'two');",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = true);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (replication_factor = 0);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (replication_factor = 1.5);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (foo = 1);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2, foo = 1);",
    ];
    for source in cases {
        let err = Parser::parse(source)
            .expect_err(&format!("expected shard key config error for: {source}"));
        assert_eq!(err.code, "QQL-PARSE-SHARD-KEY-CONFIG");
    }
}

#[test]
fn create_shard_key_config_accepts_positive_integers() {
    for source in [
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (replication_factor = 3);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2, replication_factor = 3);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (SHARDS_NUMBER = 4);",
        "CREATE SHARD KEY 'a' ON COLLECTION docs;",
    ] {
        Parser::parse(source).unwrap_or_else(|e| panic!("{source} should parse: {e}"));
    }
}

#[test]
fn create_index_unknown_field_type_rejected() {
    let err = Parser::parse("CREATE INDEX ON COLLECTION docs FOR title TYPE banana;")
        .expect_err("unknown index field type must be rejected");
    assert_eq!(err.code, "QQL-PARSE-INDEX-TYPE");
    // Case-insensitive canonical names are still accepted.
    Parser::parse("CREATE INDEX ON COLLECTION docs FOR title TYPE TEXT;")
        .unwrap_or_else(|e| panic!("canonical type should parse: {e}"));
}

#[test]
fn feedback_strategy_requires_exact_a_b_c_in_order() {
    let base =
        "QUERY RELEVANCE FEEDBACK TARGET TEXT 'x' FEEDBACK ((TEXT 'y', 1.0)) STRATEGY NAIVE ";
    let cases = [
        // reordered
        "(a = 1, c = 2, b = 3)",
        // extra parameter
        "(a = 1, b = 2, c = 3, d = 4)",
        // missing parameter
        "(a = 1, b = 2)",
        // unknown key
        "(a = 1, b = 2, x = 3)",
    ];
    for params in cases {
        let source = format!("{base}{params} FROM docs LIMIT 5;");
        let err = Parser::parse(&source)
            .expect_err(&format!("expected feedback strategy error for: {params}"));
        assert_eq!(err.code, "QQL-PARSE-FEEDBACK-STRATEGY");
    }
}

#[test]
fn rerank_bare_string_input_rejected() {
    // `rerank_input` in grammar.pest is TEXT | VECTOR | POINT only.
    let err = Parser::parse("QUERY RERANK 'x' MODEL 'm' FROM docs LIMIT 5;")
        .expect_err("bare string rerank input must be rejected");
    assert_eq!(err.code, "QQL-PARSE-RERANK");
    // IMAGE is a query input, not a rerank input.
    assert!(Parser::parse("QUERY RERANK IMAGE 'a.png' MODEL 'm' FROM docs LIMIT 5;").is_err());
}

#[test]
fn uppercase_raw_prefix_rejected_as_raw_string() {
    // grammar.pest has no uppercase `R'…'` raw form.
    let err = Parser::parse(r"QUERY R'a\nb' FROM docs LIMIT 5;")
        .expect_err("uppercase raw prefix must not lex as a raw string");
    assert_eq!(err.code, "QQL-PARSE-QUERY-INPUT");
}

#[test]
fn dollar_leading_dotted_segment_rejected() {
    // identifier_segment starts with a letter or `_` only.
    let err = Parser::parse("QUERY TEXT 'x' FROM docs WHERE a.$b = 1 LIMIT 5;")
        .expect_err("dollar-leading dotted segment must be rejected");
    assert_eq!(err.code, "QQL-LEX-CHAR");
}

#[test]
fn numeric_field_name_rejected() {
    let err = Parser::parse("QUERY TEXT 'x' FROM docs WHERE 42 = 1;")
        .expect_err("numeric field names must be rejected");
    assert_eq!(err.code, "QQL-PARSE-FIELD");
}

#[test]
fn trailing_commas_rejected() {
    for source in [
        "UPSERT INTO docs VALUES {id: 1, title: 'a',};",
        "QUERY TEXT 'x' FROM docs PARAMS (exact = true,);",
        "QUERY [0.1, 0.2,] FROM docs;",
        "CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH HNSW (m = 16,);",
    ] {
        let err = Parser::parse(source).expect_err(&format!("trailing comma: {source}"));
        assert_eq!(err.code, "QQL-PARSE-TRAILING-COMMA", "{source}");
        assert!(err.span.is_some(), "{source}");
    }
}

#[test]
fn empty_params_and_alter_without_config_rejected() {
    let err = Parser::parse("QUERY TEXT 'x' FROM docs PARAMS ();")
        .expect_err("empty PARAMS must be rejected");
    assert_eq!(err.code, "QQL-PARSE-SEARCH-PARAMS");
    assert!(err.span.is_some());

    let err =
        Parser::parse("ALTER COLLECTION docs;").expect_err("ALTER without WITH must be rejected");
    assert_eq!(err.code, "QQL-PARSE-ALTER-CONFIG");
}

#[test]
fn facet_unknown_and_duplicate_options_rejected() {
    let err = Parser::parse("FACET category FROM docs WITH (foo = 1);")
        .expect_err("unknown FACET WITH key");
    assert_eq!(err.code, "QQL-PARSE-FACET-CONFIG");

    let err = Parser::parse("FACET category FROM docs LIMIT 5 LIMIT 10;")
        .expect_err("duplicate FACET LIMIT");
    assert_eq!(err.code, "QQL-PARSE-DUPLICATE-CLAUSE");

    let err = Parser::parse("FACET category FROM docs EXACT true EXACT false;")
        .expect_err("duplicate FACET EXACT");
    assert_eq!(err.code, "QQL-PARSE-DUPLICATE-CLAUSE");

    let err = Parser::parse("FACET category FROM docs LIMIT 5 WITH (limit = 10);")
        .expect_err("WITH limit after LIMIT");
    assert_eq!(err.code, "QQL-PARSE-DUPLICATE-CLAUSE");
}

#[test]
fn between_rejects_composite_bounds() {
    let err = Parser::parse("QUERY TEXT 'x' FROM docs WHERE n BETWEEN {a: 1} AND 2;")
        .expect_err("BETWEEN dict bound");
    assert_eq!(err.code, "QQL-PARSE-LITERAL");
    assert!(err.span.is_some());
    let span = err.span.unwrap();
    assert!(
        span.end > span.start,
        "literal span must not point past the dict"
    );
}

#[test]
fn in_literal_span_points_at_the_composite() {
    let source = "QUERY TEXT 'x' FROM docs WHERE n IN ({a: 1});";
    let err = Parser::parse(source).expect_err("IN dict");
    assert_eq!(err.code, "QQL-PARSE-LITERAL");
    let span = err.span.expect("span");
    let excerpt = &source[span.start..span.end];
    assert!(excerpt.contains('{'), "span {span:?} excerpt {excerpt:?}");
}

#[test]
fn unknown_vector_config_key_is_validation() {
    let err = Parser::parse("CREATE COLLECTION docs (d VECTOR(4, COSINE) WITH VECTOR (foo = 1));")
        .expect_err("unknown VECTOR key");
    assert_eq!(err.kind, ErrorKind::Validation);
    assert!(err.span.is_some());
}

#[test]
fn hnsw_m_float_two_is_rejected() {
    let int_err = Parser::parse("CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH HNSW (m = 2);")
        .expect_err("m = 2");
    let float_err =
        Parser::parse("CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH HNSW (m = 2.0);")
            .expect_err("m = 2.0");
    assert_eq!(int_err.kind, ErrorKind::Validation);
    assert_eq!(float_err.kind, ErrorKind::Validation);
}

#[test]
fn search_param_errors_carry_spans() {
    let err = Parser::parse("QUERY TEXT 'x' FROM docs PARAMS (foo = 1);")
        .expect_err("unknown search param");
    assert_eq!(err.code, "QQL-VALIDATION-SEARCH-PARAM");
    assert!(err.span.is_some());

    let err = Parser::parse("QUERY TEXT 'x' FROM docs PARAMS (max_selectivity = 0.5);")
        .expect_err("max_selectivity without acorn");
    assert_eq!(err.code, "QQL-VALIDATION-ACORN-SELECTIVITY");
    assert!(err.span.is_some());
}

#[test]
fn empty_script_and_separator_codes() {
    assert!(Parser::parse_all("").unwrap().is_empty());
    let err = Parser::parse("").expect_err("empty single statement");
    assert_eq!(err.code, "QQL-PARSE-STATEMENT");

    let err =
        Parser::parse_all("SHOW COLLECTIONS SHOW COLLECTION docs").expect_err("missing semicolon");
    assert_eq!(err.code, "QQL-PARSE-SEPARATOR");
    assert!(err.span.is_some());

    let err = Parser::parse_all("; SHOW COLLECTIONS").expect_err("leading semicolon");
    assert_eq!(err.code, "QQL-PARSE-EMPTY-STATEMENT");
}

#[test]
fn object_operator_keys_rejected() {
    let err = Parser::parse("UPSERT INTO docs VALUES {id: 1, *: 2};").expect_err("star object key");
    assert_eq!(err.code, "QQL-PARSE-OBJECT-KEY");
}
