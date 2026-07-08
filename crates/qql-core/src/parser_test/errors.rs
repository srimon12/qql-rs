use crate::parser_test::assert_parse_err;

// ── Parse Errors ─────────────────────────────────────────────

#[test]
fn test_parse_error_invalid_statement() {
    assert_parse_err("INVALID KEYWORD");
}

#[test]
fn test_parse_error_insert_missing_values() {
    assert_parse_err("INSERT INTO test");
}

#[test]
fn test_parse_error_search_missing_query_text() {
    assert_parse_err("QUERY NEAREST FROM test");
}

#[test]
fn test_parse_error_reject_trailing_tokens() {
    assert_parse_err("INSERT INTO test VALUES {'text': 'hello'} EXTRA");
}

#[test]
fn test_parse_error_reject_explain_in_parser() {
    assert_parse_err("EXPLAIN QUERY NEAREST 'text' FROM test LIMIT 10");
}

#[test]
fn test_parse_error_reject_duplicate_where() {
    assert_parse_err("QUERY NEAREST 'text' FROM test LIMIT 10 WHERE a = 1 WHERE b = 2");
}
