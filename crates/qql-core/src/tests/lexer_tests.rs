use crate::error::Span;
use crate::lexer::Lexer;
use crate::token::TokenKind;

fn tokens(source: &str) -> Vec<(TokenKind, &str, Span)> {
    Lexer::new(source)
        .map(|r| r.unwrap())
        .map(|t| (t.kind, t.text, t.span))
        .collect()
}

#[test]
fn basic_keywords() {
    let t = tokens("QUERY UPSERT DELETE SCROLL");
    assert_eq!(t[0].0, TokenKind::Query);
    assert_eq!(t[1].0, TokenKind::Upsert);
    assert_eq!(t[2].0, TokenKind::Delete);
    assert_eq!(t[3].0, TokenKind::Scroll);
}

#[test]
fn strings_with_escapes() {
    let t = tokens(r"'hello\nworld\t'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, r"hello\nworld\t");

    let t = tokens(r"'it\'s ok'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, r"it\'s ok");
}

#[test]
fn string_span_covers_quotes() {
    let t = tokens("'hello' FROM docs");
    assert_eq!(t[0].2, Span::new(0, 7));
}

#[test]
fn numbers_integer_and_float() {
    let t = tokens("42 3.14 -5 -0.5");
    assert_eq!(t[0].0, TokenKind::Integer);
    assert_eq!(t[0].1, "42");
    assert_eq!(t[1].0, TokenKind::Float);
    assert_eq!(t[1].1, "3.14");
    assert_eq!(t[2].0, TokenKind::Integer);
    assert_eq!(t[2].1, "-5");
    assert_eq!(t[3].0, TokenKind::Float);
    assert_eq!(t[3].1, "-0.5");
}

#[test]
fn minus_as_operator() {
    let t = tokens("a - b");
    assert_eq!(t[1].0, TokenKind::Minus);
}

#[test]
fn identifiers_and_keywords() {
    let t = tokens("QUERY my_field FROM docs WHERE x = 1");
    assert_eq!(t[0].0, TokenKind::Query);
    assert_eq!(t[1].0, TokenKind::Identifier);
    assert_eq!(t[1].1, "my_field");
    assert_eq!(t[2].0, TokenKind::From);
    assert_eq!(t[3].0, TokenKind::Identifier);
    assert_eq!(t[3].1, "docs");
    assert_eq!(t[4].0, TokenKind::Where);
    assert_eq!(t[6].0, TokenKind::Equals);
}

#[test]
fn dollar_identifiers() {
    let t = tokens("$score $my_var");
    assert_eq!(t[0].0, TokenKind::Identifier);
    assert_eq!(t[0].1, "$score");
    assert_eq!(t[1].0, TokenKind::Identifier);
    assert_eq!(t[1].1, "$my_var");
}

#[test]
fn line_comments_skipped() {
    let source = "QUERY -- this is a comment\nFROM docs";
    let t = tokens(source);
    assert_eq!(t[0].0, TokenKind::Query);
    assert_eq!(t[1].0, TokenKind::From);
    assert_eq!(t[2].0, TokenKind::Identifier);
    assert_eq!(t[2].1, "docs");
}

#[test]
fn comparison_operators() {
    let t = tokens("= != > >= < <=");
    assert_eq!(t[0].0, TokenKind::Equals);
    assert_eq!(t[1].0, TokenKind::NotEquals);
    assert_eq!(t[2].0, TokenKind::Gt);
    assert_eq!(t[3].0, TokenKind::Gte);
    assert_eq!(t[4].0, TokenKind::Lt);
    assert_eq!(t[5].0, TokenKind::Lte);
}

#[test]
fn brackets_and_delimiters() {
    let t = tokens("{ } [ ] ( ) , : ;");
    assert_eq!(t[0].0, TokenKind::Lbrace);
    assert_eq!(t[1].0, TokenKind::Rbrace);
    assert_eq!(t[2].0, TokenKind::Lbracket);
    assert_eq!(t[3].0, TokenKind::Rbracket);
    assert_eq!(t[4].0, TokenKind::Lparen);
    assert_eq!(t[5].0, TokenKind::Rparen);
    assert_eq!(t[6].0, TokenKind::Comma);
    assert_eq!(t[7].0, TokenKind::Colon);
    assert_eq!(t[8].0, TokenKind::Semicolon);
}

#[test]
fn span_positions_are_bytes() {
    let source = "QUERY docs";
    let t = tokens(source);
    assert_eq!(t[0].2, Span::new(0, 5));
    assert_eq!(t[1].2, Span::new(6, 10));
}

#[test]
fn identifier_with_underscore_and_digits() {
    let t = tokens("field_name field2 _private");
    assert!(t.iter().all(|(k, _, _)| matches!(k, TokenKind::Identifier)));
}

#[test]
fn invalid_character_errors() {
    let mut lexer = Lexer::new("@invalid");
    assert!(lexer.next().unwrap().is_err());
    let mut lexer = Lexer::new("#");
    assert!(lexer.next().unwrap().is_err());
}
