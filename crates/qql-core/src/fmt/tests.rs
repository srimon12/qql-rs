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
    let input = "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs LIMIT 10;";
    let formatted = format(input).unwrap();
    assert_eq!(
        formatted,
        "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs LIMIT 10;\n"
    );
    let twice = format(&formatted).unwrap();
    assert_eq!(formatted, twice);
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
