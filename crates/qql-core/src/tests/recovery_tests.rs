//! Panic-mode recovery: multiple diagnostics, fail-fast `parse_all` unchanged.

use crate::ast::Stmt;
use crate::parser::Parser;

#[test]
fn parse_all_stays_fail_fast_on_the_first_error() {
    let err = Parser::parse_all("QUERY [0.1] FROM docs LIMIT 0; COUNT FROM docs;")
        .expect_err("LIMIT 0 is a parse error");
    assert_eq!(err.code.as_ref(), "QQL-PARSE-POSITIVE-INTEGER");
}

#[test]
fn recovering_parse_reports_later_statements_after_an_error() {
    let recovered = Parser::parse_all_recovering(
        "QUERY [0.1] FROM docs LIMIT 0; COUNT FROM other; QUERY [0.1] FROM docs LIMIT 1;",
    );
    assert!(!recovered.is_valid());
    assert_eq!(recovered.errors.len(), 1);
    assert_eq!(
        recovered.errors[0].code.as_ref(),
        "QQL-PARSE-POSITIVE-INTEGER"
    );
    assert_eq!(recovered.statements.len(), 2);
    assert!(matches!(recovered.statements[0].0, Stmt::Count(_)));
    assert!(matches!(recovered.statements[1].0, Stmt::Query(_)));
}

#[test]
fn recovering_parse_syncs_on_the_next_statement_keyword() {
    let recovered = Parser::parse_all_recovering(
        "QUERY [0.1] FROM docs LIMIT 0\nCOUNT FROM other;\nSHOW COLLECTIONS;",
    );
    assert!(!recovered.is_valid());
    assert!(
        recovered
            .errors
            .iter()
            .any(|e| e.code.as_ref() == "QQL-PARSE-POSITIVE-INTEGER")
    );
    // The first statement never completed, so COUNT is a recovery sync point
    // (not a missing-semicolon on a successful parse).
    assert_eq!(recovered.statements.len(), 2);
    assert!(matches!(recovered.statements[0].0, Stmt::Count(_)));
    assert!(matches!(recovered.statements[1].0, Stmt::ShowCollections));
}

#[test]
fn recovering_parse_reports_a_missing_semicolon_between_valid_statements() {
    let recovered = Parser::parse_all_recovering("COUNT FROM docs\nSHOW COLLECTIONS;");
    assert!(!recovered.is_valid());
    assert_eq!(recovered.errors.len(), 1);
    assert_eq!(recovered.errors[0].code.as_ref(), "QQL-PARSE-SEPARATOR");
    assert_eq!(recovered.statements.len(), 2);
    assert!(matches!(recovered.statements[0].0, Stmt::Count(_)));
    assert!(matches!(recovered.statements[1].0, Stmt::ShowCollections));
}

#[test]
fn recovering_parse_collects_two_independent_syntax_errors() {
    let recovered = Parser::parse_all_recovering(
        "QUERY [0.1] FROM docs LIMIT 0; SHOW COLLECTIONS; QUERY [0.2] FROM other LIMIT 0;",
    );
    assert_eq!(recovered.errors.len(), 2);
    assert!(
        recovered
            .errors
            .iter()
            .all(|e| e.code.as_ref() == "QQL-PARSE-POSITIVE-INTEGER")
    );
    assert_eq!(recovered.statements.len(), 1);
    assert!(matches!(recovered.statements[0].0, Stmt::ShowCollections));
}

#[test]
fn recovering_parse_valid_script_matches_parse_all() {
    let src = "COUNT FROM docs; SHOW COLLECTIONS;";
    let recovered = Parser::parse_all_recovering(src);
    assert!(recovered.is_valid());
    let fail_fast = Parser::parse_all(src).expect("valid script");
    assert_eq!(recovered.statements.len(), fail_fast.len());
}

#[test]
fn recovering_parse_does_not_split_with_payload_as_a_new_statement() {
    let recovered =
        Parser::parse_all_recovering("QUERY [0.1] FROM docs WITH PAYLOAD false LIMIT 1;");
    assert!(
        recovered.is_valid(),
        "WITH PAYLOAD is a clause, not a sync token: {:?}",
        recovered.errors
    );
    assert_eq!(recovered.statements.len(), 1);
}
