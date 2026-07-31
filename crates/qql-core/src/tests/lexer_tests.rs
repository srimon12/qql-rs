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
fn grammar_keywords_in_token_rs() {
    let pest = include_str!("../../../../language/v1/grammar.pest");
    for line in pest.lines() {
        let line = line.split("//").next().unwrap_or_default();
        let mut rest = line;
        while let Some(idx) = rest.find("^\"") {
            let cand = &rest[idx + 2..];
            let end = cand.find('"').unwrap();
            let word = &cand[..end];
            if word.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') && word.len() > 1 {
                let kw = word.to_ascii_uppercase();
                assert!(
                    crate::token::lookup_keyword(&kw).is_some(),
                    "Keyword '{}' from grammar.pest is missing from token.rs KEYWORDS map",
                    kw
                );
            }
            rest = &cand[end + 1..];
        }
    }
}

#[test]
fn parser_keywords_exist_in_grammar() {
    use std::fs;
    use std::path::Path;

    let parser_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/parser");
    let mut words = std::collections::BTreeSet::new();

    fn scan_dir(dir: &Path, words: &mut std::collections::BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, words);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let code = fs::read_to_string(&path).unwrap();
                for line in code.lines() {
                    for pattern in &[
                        "ascii_equal(",
                        "peek_word(",
                        "expect_word(",
                        "eq_ignore_ascii_case(",
                    ] {
                        // `peek_word`/`expect_word` compare token text by
                        // construction. `ascii_equal`/`eq_ignore_ascii_case`
                        // are also used on parsed *values* (config keys, string
                        // payloads), which are semantic domains documented in
                        // `spec/semantics.md` — not grammar keywords. Only a
                        // token-text receiver proves the word sits in a
                        // syntactic keyword position.
                        let receiver_gated = pattern.starts_with("ascii_equal(")
                            || pattern.starts_with("eq_ignore_ascii_case(");
                        let mut rest = line;
                        while let Some(pos) = rest.find(pattern) {
                            if receiver_gated {
                                let after_paren = &rest[pos + pattern.len()..];
                                let receiver_end =
                                    after_paren.find(',').unwrap_or(after_paren.len());
                                let receiver = &after_paren[..receiver_end];
                                if !receiver.contains(".text") {
                                    rest = &rest[pos + pattern.len()..];
                                    continue;
                                }
                            }
                            let candidate = &rest[pos + pattern.len()..];
                            if let Some(start_quote) = candidate.find('"') {
                                let after_quote = &candidate[start_quote + 1..];
                                if let Some(end_quote) = after_quote.find('"') {
                                    let word = &after_quote[..end_quote];
                                    if word.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_')
                                        && word.len() > 1
                                    {
                                        words.insert(word.to_ascii_uppercase());
                                    }
                                }
                            }
                            rest = &candidate[1..];
                        }
                    }
                }
            }
        }
    }

    scan_dir(&parser_dir, &mut words);

    assert!(!words.is_empty(), "expected parser string keyword calls");
    for word in &words {
        assert!(
            crate::token::lookup_keyword(word).is_some(),
            "Parser checks word '{word}' which is NOT in token.rs KEYWORDS map (or grammar.pest)"
        );
    }
}

#[test]
fn unicode_comparison_operators() {
    let t = tokens("year ≥ 2024 AND year ≤ 2030 AND id ≠ 5");
    assert_eq!(t[1].0, TokenKind::Gte);
    assert_eq!(t[5].0, TokenKind::Lte);
    assert_eq!(t[9].0, TokenKind::NotEquals);
}

#[test]
fn strings_with_escapes() {
    let t = tokens(r"'hello\nworld\t'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, r"hello\nworld\t");

    let t = tokens(r"'it\'s ok'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, r"it\'s ok");

    let t = tokens(r"'price is \$100'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, r"price is \$100");
}

#[test]
fn raw_and_triple_quoted_and_backtick_strings() {
    let t = tokens(r"r'QUERY $QUERY_TEXT FROM docs USING dense LIMIT $LIMIT;'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(
        t[0].1,
        r"QUERY $QUERY_TEXT FROM docs USING dense LIMIT $LIMIT;"
    );

    let t = tokens("'''QUERY '$QUERY_TEXT'\nFROM berlin_airbnb\nLIMIT $LIMIT;'''");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(
        t[0].1,
        "QUERY '$QUERY_TEXT'\nFROM berlin_airbnb\nLIMIT $LIMIT;"
    );

    let t = tokens("`QUERY '$QUERY_TEXT'`");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, "QUERY '$QUERY_TEXT'");
}

#[test]
fn string_span_covers_quotes() {
    let t = tokens("'hello' FROM docs");
    assert_eq!(t[0].2, Span::new(0, 7));
}

#[test]
fn scientific_notation_floats() {
    let t = tokens("1e-5 1.2e+3 1.0e-05 -5e2");
    assert_eq!(t[0].0, TokenKind::Float);
    assert_eq!(t[0].1, "1e-5");
    assert_eq!(t[1].0, TokenKind::Float);
    assert_eq!(t[1].1, "1.2e+3");
    assert_eq!(t[2].0, TokenKind::Float);
    assert_eq!(t[2].1, "1.0e-05");
    assert_eq!(t[3].0, TokenKind::Float);
    assert_eq!(t[3].1, "-5e2");
}

#[test]
fn sql_style_escaped_strings() {
    let t = tokens("'St. Peter''s Church'");
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, "St. Peter''s Church");
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
