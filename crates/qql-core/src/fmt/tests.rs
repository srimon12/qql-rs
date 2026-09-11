use super::*;
use crate::fmt::expr::render_f32;

#[test]
fn test_render_f32_precision() {
    assert_eq!(render_f32(0.1), "0.1");
    assert_eq!(render_f32(0.2), "0.2");
    assert_eq!(render_f32(0.3), "0.3");
    assert_eq!(render_f32(1.0), "1.0");
    assert_eq!(render_f32(0.0), "0.0");
    assert_eq!(render_f32(-0.5), "-0.5");
}

#[test]
fn test_vector_formatting_precision() {
    // Compact vector inputs are the canonical spelling (`allow_bare` is
    // honored for `QueryInput::Vector`, matching the language reference).
    let input = "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs LIMIT 10;";
    let formatted = format(input).unwrap();
    assert_eq!(formatted, "QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 10;\n");
    let twice = format(&formatted).unwrap();
    assert_eq!(formatted, twice);
}

#[test]
fn test_recommend_example_inputs_roundtrip() {
    // Regression: `render_recommend_input` emitted `VECTOR […]` where the
    // parser only accepted a point-id list, so the formatted text could not
    // be parsed back at all. Both spellings must now round-trip.
    for source in [
        "QUERY RECOMMEND POSITIVE ([0.1, 0.2]) FROM docs LIMIT 5;",
        "QUERY RECOMMEND POSITIVE (VECTOR [0.1, 0.2]) FROM docs LIMIT 5;",
        "QUERY RECOMMEND POSITIVE (TEXT 'hello') NEGATIVE (2) FROM docs LIMIT 5;",
        "QUERY RECOMMEND POSITIVE (1, 'pt') STRATEGY best_score FROM docs LIMIT 5;",
    ] {
        let parsed = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
        let formatted = format_stmt(&parsed);
        let reparsed =
            Parser::parse(&formatted).unwrap_or_else(|e| panic!("reparse {formatted}: {e}"));
        assert_eq!(format_stmt(&reparsed), formatted, "not canonical: {source}");
    }
    let vector =
        format_stmt(&Parser::parse("QUERY RECOMMEND POSITIVE ([0.1]) FROM docs LIMIT 5;").unwrap());
    assert_eq!(
        vector,
        "QUERY RECOMMEND POSITIVE (VECTOR [0.1]) FROM docs LIMIT 5"
    );
}

#[test]
fn test_format_preserves_comments_and_blank_lines() {
    let input = r#"-- Header comment line 1
-- Header comment line 2

-- Section 1
COUNT FROM docs;

-- Section 2
COUNT FROM docs WHERE status = 'active'; -- trailing comment
"#;
    let formatted = format(input).unwrap();
    assert_eq!(formatted, input);
    let twice = format(&formatted).unwrap();
    assert_eq!(formatted, twice);
}

#[test]
fn test_format_collapses_multiple_blank_lines() {
    let input = r#"COUNT FROM docs;




COUNT FROM other;
"#;
    let formatted = format(input).unwrap();
    let expected = "COUNT FROM docs;\n\nCOUNT FROM other;\n";
    assert_eq!(formatted, expected);
    let twice = format(&formatted).unwrap();
    assert_eq!(formatted, twice);
}

#[test]
fn test_format_inline_comment_inside_statement() {
    let input =
        "QUERY 'search' FROM docs\n-- filter status\nWHERE status = 'published' LIMIT 10;\n";
    let formatted = format(input).unwrap();
    let expected =
        "-- filter status\nQUERY 'search' FROM docs WHERE status = 'published' LIMIT 10;\n";
    assert_eq!(formatted, expected);
    let twice = format(&formatted).unwrap();
    assert_eq!(formatted, twice);
}

#[test]
fn test_shard_key_forms_roundtrip() {
    // Keyword stays quoted, numbers stay bare, placeholders stay placeholders —
    // the formatter must never coerce a numeric key into a keyword string.
    for input in [
        "QUERY TEXT 'x' FROM docs SHARD 'acme' LIMIT 1;",
        "DELETE FROM docs WHERE id = 1 SHARD 101;",
        "QUERY TEXT 'x' FROM docs SHARD :tenant LIMIT 1;",
        "DROP SHARD KEY 101 ON COLLECTION docs;",
        "CREATE SHARD KEY 101 ON COLLECTION docs;",
    ] {
        let formatted = format(input).unwrap();
        assert!(
            formatted.contains("SHARD"),
            "shard clause lost for {input}: {formatted}"
        );
        let twice = format(&formatted).unwrap();
        assert_eq!(formatted, twice, "format not idempotent for {input}");
    }
    let numeric = format("DELETE FROM docs WHERE id = 1 SHARD 101;").unwrap();
    assert!(
        numeric.contains("SHARD 101;"),
        "numeric key must render bare, got: {numeric}"
    );
    assert!(
        !numeric.contains("SHARD '101'"),
        "numeric key must not be quoted, got: {numeric}"
    );
}
