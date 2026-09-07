use super::*;
use crate::ast::Value;
use crate::parser::Parser;
use alloc::vec;

fn check_value_roundtrip(val: Value) {
    let lit = value_to_literal(&val).expect("value_to_literal should succeed");
    let parsed = Parser::parse_value(&lit).expect("Parser::parse_value should succeed");
    assert_eq!(
        parsed, val,
        "value roundtrip mismatch for literal '{}'",
        lit
    );
}

#[test]
fn test_value_to_literal_escaping() {
    assert_eq!(
        value_to_literal(&Value::Str("O'Connor and \\path".into())).unwrap(),
        "'O\\'Connor and \\\\path'"
    );
    assert_eq!(value_to_literal(&Value::Int(42)).unwrap(), "42");
    assert_eq!(value_to_literal(&Value::Float(3.75)).unwrap(), "3.75");
    assert_eq!(value_to_literal(&Value::Float(1.0)).unwrap(), "1.0");
    assert_eq!(value_to_literal(&Value::Float(1e300)).unwrap(), "1e300");
    assert_eq!(value_to_literal(&Value::Bool(true)).unwrap(), "true");
    assert_eq!(value_to_literal(&Value::Null).unwrap(), "null");
}

#[test]
fn test_value_ast_equality_roundtrips() {
    check_value_roundtrip(Value::Str("plain".into()));
    check_value_roundtrip(Value::Str(
        "escaped 'quote' and \\backslash\nnewline".into(),
    ));
    check_value_roundtrip(Value::Int(12345));
    check_value_roundtrip(Value::Float(45.5));
    check_value_roundtrip(Value::Float(1.25e-5));
    check_value_roundtrip(Value::Bool(true));
    check_value_roundtrip(Value::Bool(false));
    check_value_roundtrip(Value::Null);
    check_value_roundtrip(Value::List(alloc::vec![
        Value::Int(1),
        Value::Str("two".into()),
        Value::Bool(true),
    ]));
    check_value_roundtrip(Value::Dict(alloc::vec![
        ("simple".into(), Value::Int(1)),
        ("".into(), Value::Str("empty key".into())),
        ("$special_ident".into(), Value::Int(2)),
        ("foo.bar".into(), Value::Int(3)),
        ("a: 1, b".into(), Value::Int(4)),
        ("weird 'quoted'".into(), Value::Bool(false)),
    ]));
}

#[test]
fn test_parse_value_rejects_trailing_tokens() {
    assert!(Parser::parse_value("42 trailing").is_err());
    assert!(Parser::parse_value("'string' extra").is_err());
}

#[test]
fn test_dict_key_escaping_prevents_injection() {
    let dict = Value::Dict(alloc::vec![
        ("simple_key".into(), Value::Int(1)),
        ("a: 1, b".into(), Value::Int(5)),
        ("weird 'quote".into(), Value::Str("val".into())),
    ]);
    let lit = value_to_literal(&dict).unwrap();
    assert_eq!(
        lit,
        "{simple_key: 1, 'a: 1, b': 5, 'weird \\'quote': 'val'}"
    );

    let query = format!("UPSERT INTO test VALUES {{id: 1, payload: {}}};", lit);
    let parsed = Parser::parse(&query);
    assert!(parsed.is_ok(), "parsed error: {:?}", parsed.err());
}

#[test]
fn test_non_finite_floats_rejected() {
    assert!(value_to_literal(&Value::Float(f64::NAN)).is_err());
    assert!(value_to_literal(&Value::Float(f64::INFINITY)).is_err());
    assert!(value_to_literal(&Value::Float(f64::NEG_INFINITY)).is_err());
}

#[test]
fn test_dollar_identifiers_never_corrupted() {
    let query = "QUERY TEXT 'chest pain' FROM docs WHERE $category = 'medical' AND $1 = 5 LIMIT 5;";
    let bound = bind_named(query, |name| match name {
        "category" => Some(Value::Str("ignored".into())),
        _ => None,
    })
    .unwrap();

    assert_eq!(bound, query);
}

#[test]
fn test_bind_preserves_compact_dict_syntax() {
    // In {id: 1, a:b} or {'a':b}, the colon after 'a' or ''a'' is a dictionary separator, NOT a placeholder :b.
    let query = "UPSERT INTO t VALUES {id: 1, a:b, 'c':d, \"e\":f, `g`:h};";
    let bound = bind_named(query, |name| match name {
        "b" | "d" | "f" | "h" => Some(Value::Int(99)),
        _ => None,
    })
    .unwrap();

    assert_eq!(bound, query);

    // Positional binder must also ignore compact dict colons and not flag false-positive mixed style
    let query_pos = "UPSERT INTO t VALUES {id: ?, a:b};";
    let bound_pos = bind_positional(query_pos, &[Value::Int(1)]).unwrap();
    assert_eq!(bound_pos, "UPSERT INTO t VALUES {id: 1, a:b};");
}

#[test]
fn test_bind_named_variables() {
    let query = "QUERY TEXT :q FROM docs WHERE category = :cat AND active = :is_active LIMIT :lim;";
    let result = bind_named(query, |name| match name {
        "q" => Some(Value::Str("chest pain".into())),
        "cat" => Some(Value::Str("medical".into())),
        "is_active" => Some(Value::Bool(true)),
        "lim" => Some(Value::Int(10)),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        result,
        "QUERY TEXT 'chest pain' FROM docs WHERE category = 'medical' AND active = true LIMIT 10;"
    );
}

#[test]
fn test_mixed_placeholder_style_errors() {
    let query_with_q = "QUERY TEXT :q FROM docs LIMIT ?;";
    assert!(bind_named(query_with_q, |_| Some(Value::Str("x".into()))).is_err());

    let query_with_name = "QUERY TEXT ? FROM docs WHERE cat = :cat;";
    assert!(bind_positional(query_with_name, &[Value::Str("x".into())]).is_err());
}

#[test]
fn test_bind_preserves_literals_comments_and_backticks() {
    let query = "-- Search for :cat in comments\nQUERY TEXT 'hello :q' FROM docs WHERE path = `C:\\docs\\:name` AND status = :status;";
    let result = bind_named(query, |name| match name {
        "status" => Some(Value::Str("active".into())),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        result,
        "-- Search for :cat in comments\nQUERY TEXT 'hello :q' FROM docs WHERE path = `C:\\docs\\:name` AND status = 'active';"
    );
}

#[test]
fn test_bind_positional_variables() {
    let query = "QUERY TEXT ? FROM docs WHERE tenant = ? AND score >= ? LIMIT ?;";
    let params = alloc::vec![
        Value::Str("acme".into()),
        Value::Str("acme_tenant".into()),
        Value::Float(0.85),
        Value::Int(5),
    ];
    let result = bind_positional(query, &params).unwrap();
    assert_eq!(
        result,
        "QUERY TEXT 'acme' FROM docs WHERE tenant = 'acme_tenant' AND score >= 0.85 LIMIT 5;"
    );
}

#[test]
fn test_bind_positional_count_mismatch() {
    let query = "QUERY TEXT ? FROM docs LIMIT ?;";
    let too_few = alloc::vec![Value::Str("q".into())];
    assert!(bind_positional(query, &too_few).is_err());

    let too_many = alloc::vec![Value::Str("q".into()), Value::Int(5), Value::Int(10)];
    assert!(bind_positional(query, &too_many).is_err());

    let query_no_placeholders = "QUERY TEXT 'test' FROM docs LIMIT 5;";
    let err = bind_positional(query_no_placeholders, &[Value::Int(1)]).unwrap_err();
    assert!(
        err.to_string()
            .contains("no '?' placeholders found in query")
    );
}

#[test]
fn test_dotted_parameter_names() {
    let query =
        "QUERY TEXT :center.query FROM docs WHERE lat = :center.lat AND lon = :center.lon LIMIT 5;";
    let result = bind_named(query, |name| match name {
        "center.query" => Some(Value::Str("coffee".into())),
        "center.lat" => Some(Value::Float(37.7749)),
        "center.lon" => Some(Value::Float(-122.4194)),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        result,
        "QUERY TEXT 'coffee' FROM docs WHERE lat = 37.7749 AND lon = -122.4194 LIMIT 5;"
    );
}

#[test]
fn test_bind_stmt_ast() {
    let query = "QUERY TEXT :q FROM docs WHERE category = :cat AND score > :min_score LIMIT :lim;";
    let mut stmt = Parser::parse(query).expect("query with parameters should parse into AST");

    bind_stmt(
        &mut stmt,
        |name| match name {
            "q" => Some(Value::Str("headache".into())),
            "cat" => Some(Value::Str("medical".into())),
            "min_score" => Some(Value::Float(0.75)),
            "lim" => Some(Value::Int(10)),
            _ => None,
        },
        &[],
    )
    .expect("bind_stmt should succeed");

    let formatted = crate::fmt::format_stmt(&stmt);
    assert_eq!(
        formatted,
        "QUERY 'headache' FROM docs WHERE category = 'medical' AND score > 0.75 LIMIT 10"
    );
}

#[test]
fn test_truncate_vector_literals() {
    let qql = "QUERY VECTOR [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] FROM docs LIMIT 5;";
    let truncated = truncate_vector_literals(qql, 3);
    assert_eq!(
        truncated,
        "QUERY VECTOR [0.1, 0.2, 0.3, ... (8 dims)] FROM docs LIMIT 5;"
    );

    // String literals inside queries are preserved
    let with_str = "QUERY TEXT '[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]' FROM docs LIMIT 5;";
    let not_truncated = truncate_vector_literals(with_str, 3);
    assert_eq!(not_truncated, with_str);
}

#[test]
fn test_bind_preserves_triple_quoted_and_raw_strings() {
    let query =
        r#"QUERY TEXT """hello :not_a_param""" FROM docs WHERE raw = r'foo\:bar' AND val = :val;"#;
    let result = bind_named(query, |name| match name {
        "val" => Some(Value::Int(42)),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        result,
        r#"QUERY TEXT """hello :not_a_param""" FROM docs WHERE raw = r'foo\:bar' AND val = 42;"#
    );
}

#[test]
fn test_scroll_and_facet_limit_and_after_binding() {
    let scroll_query = "SCROLL FROM docs AFTER :cursor LIMIT :lim;";
    let mut scroll_stmt = Parser::parse(scroll_query).expect("scroll query should parse");
    bind_stmt(
        &mut scroll_stmt,
        |name| match name {
            "cursor" => Some(Value::Str("pt-100".into())),
            "lim" => Some(Value::Int(50)),
            _ => None,
        },
        &[],
    )
    .expect("bind_stmt should bind scroll after and limit");

    validate_no_unbound_params(&scroll_stmt).expect("scroll should have no unbound params");

    let facet_query = "FACET category FROM docs LIMIT :lim;";
    let mut facet_stmt = Parser::parse(facet_query).expect("facet query should parse");
    bind_stmt(
        &mut facet_stmt,
        |name| match name {
            "lim" => Some(Value::Int(15)),
            _ => None,
        },
        &[],
    )
    .expect("bind_stmt should bind facet limit");

    validate_no_unbound_params(&facet_stmt).expect("facet should have no unbound params");
}

#[test]
fn test_query_text_colon_literal_not_unbound_param() {
    let query = "QUERY TEXT ':heart:' FROM docs LIMIT 5;";
    let stmt = Parser::parse(query).expect("query with literal colon should parse");
    // validate_no_unbound_params must not treat ':heart:' literal as an unbound param
    validate_no_unbound_params(&stmt).expect("literal ':heart:' is not an unbound param");
}

#[test]
fn test_validate_no_unbound_params_catches_missing() {
    let query = "QUERY TEXT :q FROM docs LIMIT 5;";
    let stmt = Parser::parse(query).expect("query should parse");
    let err = validate_no_unbound_params(&stmt).expect_err("should detect unbound :q");
    assert_eq!(err.code, "QQL-BIND-MISSING-PARAM");
}

// ── Formatting round-trips for parameter placeholders ────────────────

fn assert_format_reparses(query: &str, label: &str) {
    let stmt = Parser::parse(query).unwrap_or_else(|e| panic!("{label}: parse: {e}"));
    let formatted = crate::fmt::format_stmt(&stmt);
    Parser::parse(&formatted)
        .unwrap_or_else(|e| panic!("{label}: format output does not re-parse: {e}\n{formatted}"));
}

#[test]
fn test_format_positional_limit_is_bare_question_mark() {
    // `LIMIT ?` stores index 0 — formatting must render a bare `?`, not
    // `?0`/`?1`, which would re-parse as a param plus a stray integer.
    assert_format_reparses("QUERY [0.1] FROM docs LIMIT ?;", "positional LIMIT");
    assert_format_reparses(
        "QUERY [0.1] FROM docs LIMIT 5 OFFSET ?;",
        "positional OFFSET",
    );
    assert_format_reparses(
        "QUERY [0.1] FROM docs WHERE x = ? LIMIT ?;",
        "positional LIMIT",
    );
    assert_format_reparses(
        "QUERY FORMULA GAUSS_DECAY(age_days, TARGET = ?) FROM docs;",
        "positional formula target",
    );
    assert_format_reparses("QUERY TEXT ? FROM docs;", "positional TEXT");
    assert_format_reparses(
        "QUERY HYBRID TEXT ? DENSE dense SPARSE bm25 FUSION RRF FROM docs;",
        "positional HYBRID TEXT",
    );
    assert_format_reparses(
        "WITH candidates AS (QUERY 'search query' USING dense LIMIT 100) QUERY CROSS RERANK TEXT ? MODEL 'bge-reranker-large' FROM docs PREFETCH (candidates);",
        "positional CROSS RERANK",
    );
    assert_format_reparses("SCROLL FROM docs AFTER ? LIMIT ?;", "positional SCROLL");
}

#[test]
fn test_format_named_scroll_and_facet_limit_params() {
    // ScrollStmt.limit is a plain u64 with a default — formatting must
    // render the placeholder, not `LIMIT <default>` / `LIMIT None`.
    assert_format_reparses("SCROLL FROM docs LIMIT :lim;", "named SCROLL LIMIT");
    assert_format_reparses("FACET category FROM docs LIMIT :lim;", "named FACET LIMIT");
    assert_format_reparses(
        "FACET category FROM docs LIMIT ?;",
        "positional FACET LIMIT",
    );
}

#[test]
fn test_format_query_limit_param_roundtrip() {
    assert_format_reparses("QUERY [0.1] FROM docs LIMIT :lim;", "named LIMIT");
    assert_format_reparses(
        "QUERY [0.1] FROM docs LIMIT :lim OFFSET :off;",
        "named LIMIT+OFFSET",
    );
}

#[test]
fn test_bound_vector_rejects_non_finite() {
    let mut stmt = Parser::parse("QUERY :q FROM docs;").expect("should parse");
    // An f64 source beyond f32::MAX (e.g. JSON `1e39`) overflows to
    // infinity when converted — must reject like the textual parse path
    // (QQL-VALIDATION-VECTOR).
    let err = bind_stmt(
        &mut stmt,
        |name| {
            if name == "q" {
                Some(Value::List(vec![Value::Float(1e39)]))
            } else {
                None
            }
        },
        &[],
    )
    .expect_err("inf vector element must be rejected");
    assert_eq!(err.code, "QQL-VALIDATION-VECTOR");
}

#[test]
fn test_bound_zero_limit_rejected_like_literal() {
    for query in [
        "QUERY [0.1] FROM docs LIMIT :lim;",
        "SCROLL FROM docs LIMIT :lim;",
        "FACET category FROM docs LIMIT :lim;",
    ] {
        let mut stmt = Parser::parse(query).expect("should parse");
        let err = bind_stmt(
            &mut stmt,
            |name| {
                if name == "lim" {
                    Some(Value::Int(0))
                } else {
                    None
                }
            },
            &[],
        )
        .expect_err("bound LIMIT 0 must fail like the literal form");
        assert_eq!(err.code, "QQL-BIND-TYPE-MISMATCH", "{query}");
        assert!(err.span.is_some(), "bound LIMIT 0 must carry source span");
        // OFFSET 0 stays valid.
        let mut stmt =
            Parser::parse("QUERY [0.1] FROM docs LIMIT 5 OFFSET :off;").expect("should parse");
        bind_stmt(
            &mut stmt,
            |name| {
                if name == "off" {
                    Some(Value::Int(0))
                } else {
                    None
                }
            },
            &[],
        )
        .expect("bound OFFSET 0 is valid");
    }
}
