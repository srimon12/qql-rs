mod lexer_tests;
mod filter_tests;
mod parser_tests;
mod ddl_tests;
mod negative_tests;
mod transform_tests;

pub(crate) use crate::ast::Value;
pub(crate) use crate::parser::Parser;

#[test]
fn errors_have_explicit_kinds_codes_and_spans() {
    use crate::error::{ErrorKind, QqlError, Span};
    use crate::lexer::Lexer;

    let parse = Parser::parse("SELECT").expect_err("SELECT is not a statement");
    assert_eq!(parse.kind, ErrorKind::Parse);
    assert_eq!(parse.span, Some(Span::new(0, 6)));

    let lex = Parser::parse("@").expect_err("invalid character must fail lexing");
    assert_eq!(lex.kind, ErrorKind::Lex);
    assert_eq!(lex.span, Some(Span::new(0, 1)));

    let token = Lexer::new("QUERY").next().unwrap().unwrap();
    assert_eq!(token.span, Span::new(0, 5));

    let exec = QqlError::execution("TEST", "exec", None);
    assert_eq!(exec.kind, ErrorKind::Execution);
    let transport = QqlError::transport("TEST", "transport", None);
    assert_eq!(transport.kind, ErrorKind::Transport);
    let backend = QqlError::backend("TEST", "backend", None);
    assert_eq!(backend.kind, ErrorKind::Backend);
}

#[test]
fn explain_reports_ast_intent_without_runtime_claims() {
    use crate::explain;
    let output = explain::explain("QUERY NEAREST POINT 1 FROM docs;").unwrap();
    assert!(output.contains("nearest neighbors from a point"));
    assert!(!output.contains("ColBERT"));
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
