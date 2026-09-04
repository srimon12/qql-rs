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
fn iterator_terminates_after_a_lex_error() {
    // `read_number` returns Err without advancing `pos` (e.g. a trailing '.'
    // after `1.`). The iterator must surface that error exactly once and
    // then stop — an iterator that re-yields the same item forever hangs
    // any `flatten()`-style consumer.
    for source in [
        "COUNT FROM docs WHERE score >= 1.;",
        "COUNT FROM docs WHERE score >= 1e+;",
        "QUERY 'unterminated",
    ] {
        let mut results = Vec::new();
        for token in Lexer::new(source) {
            results.push(token);
            assert!(
                results.len() <= 4096,
                "lexer iterator did not terminate for: {source}"
            );
        }
        let errors = results.iter().filter(|r| r.is_err()).count();
        assert_eq!(errors, 1, "expected exactly one lex error for: {source}");
    }
}

#[test]
fn basic_keywords() {
    let t = tokens("QUERY UPSERT DELETE SCROLL");
    assert_eq!(t[0].0, TokenKind::Query);
    assert_eq!(t[1].0, TokenKind::Upsert);
    assert_eq!(t[2].0, TokenKind::Delete);
    assert_eq!(t[3].0, TokenKind::Scroll);
}

/// Canonical grammar keyword literals (`^"word"`, ASCII words, length > 1).
/// Returns `None` when the grammar file is unavailable (e.g. when qql-core is
/// tested standalone from a published crate), so drift tests skip instead of
/// failing to compile (the old `include_str!` broke packaged tests).
///
/// The path resolves relative to `CARGO_MANIFEST_DIR` inside the qql-rs
/// workspace: `crates/qql-core` → `<workspace>/language/v1/grammar.pest`.
/// `cargo package` verifies in a detached crate copy where that path does not
/// exist, so a packaged standalone test run skips here while the workspace
/// (CI) run exercises the lockstep checks against the canonical grammar.
fn grammar_keyword_literals() -> Option<std::collections::BTreeSet<String>> {
    use std::path::Path;

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/v1/grammar.pest");
    let source = std::fs::read_to_string(path).ok()?;
    let mut words = std::collections::BTreeSet::new();
    for line in source.lines() {
        let line = line.split("//").next().unwrap_or_default();
        let mut rest = line;
        while let Some(idx) = rest.find("^\"") {
            let cand = &rest[idx + 2..];
            let Some(end) = cand.find('"') else {
                break;
            };
            let word = &cand[..end];
            if word.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') && word.len() > 1 {
                words.insert(word.to_ascii_uppercase());
            }
            rest = &cand[end + 1..];
        }
    }
    Some(words)
}

#[test]
fn grammar_keywords_in_token_rs() {
    let Some(grammar_words) = grammar_keyword_literals() else {
        eprintln!("skipping: language/v1/grammar.pest is not present");
        return;
    };
    assert!(
        !grammar_words.is_empty(),
        "expected grammar keyword literals"
    );
    for kw in &grammar_words {
        assert!(
            crate::token::lookup_keyword(kw).is_some(),
            "Keyword '{kw}' from grammar.pest is missing from token.rs KEYWORDS map"
        );
    }
}

#[test]
fn keywords_map_matches_grammar_literals_exactly() {
    // Exact bidirectional keyword check (F-9): the generated `KEYWORDS` map
    // must be exactly the grammar's keyword literal set, in both directions.
    let Some(grammar_words) = grammar_keyword_literals() else {
        eprintln!("skipping: language/v1/grammar.pest is not present");
        return;
    };
    let map_keys: std::collections::BTreeSet<String> = crate::token::KEYWORDS
        .keys()
        .map(|key| (*key).to_string())
        .collect();
    assert_eq!(
        grammar_words, map_keys,
        "grammar.pest keyword literals and the generated KEYWORDS map must match exactly"
    );
}

#[test]
fn every_keyword_variant_maps_back_to_its_grammar_literal() {
    // Every KEYWORDS entry must resolve to a TokenKind whose `as_str()`
    // matches the grammar literal it was generated from (case-insensitive),
    // so a hand-edit of token.rs cannot silently rename a keyword.
    for (literal, variant) in crate::token::KEYWORDS.entries() {
        let canonical = variant.as_str();
        assert!(
            canonical.eq_ignore_ascii_case(literal),
            "KEYWORDS entry '{literal}' resolves to a variant whose as_str() is '{canonical}'"
        );
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
fn four_quotes_lex_as_single_quoted_escaped_string() {
    // `''''` is `'` + `''` + `'` (one escaped apostrophe), not a triple-quote.
    let t = tokens("''''");
    assert_eq!(t.len(), 1, "expected exactly one token: {:?}", t);
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, "''");
    assert_eq!(t[0].2, Span::new(0, 4));
}

#[test]
fn four_double_quotes_lex_as_two_empty_strings() {
    // `""""` is `""` + `""` (two empty double-quoted strings).
    let t = tokens("\"\"\"\"");
    assert_eq!(t.len(), 2);
    assert!(t
        .iter()
        .all(|(kind, text, _)| { *kind == TokenKind::String && text.is_empty() }));
}

#[test]
fn six_quotes_lex_as_empty_triple_quoted_string() {
    let t = tokens("''''''");
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, "");
    assert_eq!(t[0].2, Span::new(0, 6));
}

#[test]
fn triple_quoted_string_lexes_verbatim_content() {
    let t = tokens(r"'''a\nb'''");
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, TokenKind::String);
    assert_eq!(t[0].1, r"a\nb");
}

#[test]
fn uppercase_raw_prefix_is_not_a_raw_string() {
    // grammar.pest `raw_string` is lowercase `r` only; `R'…'` must lex as
    // the identifier `R` followed by a string.
    let t = tokens(r"R'a\nb'");
    assert_eq!(t[0].0, TokenKind::Identifier);
    assert_eq!(t[0].1, "R");
    assert_eq!(t[1].0, TokenKind::String);
    assert_eq!(t[1].1, r"a\nb");
}

#[test]
fn dotted_segment_cannot_start_with_dollar() {
    // identifier_segment starts with a letter or `_` only; `.` followed by
    // `$` must not extend the identifier.
    let mut lexer = Lexer::new("a.$b");
    let first = lexer.next().unwrap().unwrap();
    assert_eq!(first.kind, TokenKind::Identifier);
    assert_eq!(first.text, "a");
    assert!(
        lexer.next().unwrap().is_err(),
        "`.` before `$b` must be an error"
    );
}

#[test]
fn dollar_is_allowed_inside_dotted_segment() {
    // identifier_char includes `$`, so `a.b$c` is a single identifier.
    let t = tokens("a.b$c");
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, TokenKind::Identifier);
    assert_eq!(t[0].1, "a.b$c");
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
fn comment_edge_cases_are_skipped() {
    // `--` comment as the first token, at EOF without a newline, and inline
    // before the statement terminator are all consumed like WHITESPACE.
    let t = tokens("-- leading comment\nSHOW COLLECTIONS");
    assert_eq!(t[0].0, TokenKind::Show);
    assert_eq!(t[1].0, TokenKind::Collections);

    let t = tokens("SHOW COLLECTIONS; -- trailing no newline");
    assert_eq!(t[0].0, TokenKind::Show);
    assert_eq!(t[2].0, TokenKind::Semicolon);
    assert!(t.len() == 3, "comment must not produce a token: {:?}", t);

    let t = tokens("-- comment only");
    assert!(t.is_empty());

    let t = tokens("SHOW -- inline comment\nCOLLECTIONS");
    assert_eq!(t[0].0, TokenKind::Show);
    assert_eq!(t[1].0, TokenKind::Collections);
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

#[test]
fn malformed_exponent_is_a_structured_lex_error() {
    // grammar `exponent` = (^"e" | ^"E") ~ ("+" | "-")? ~ digit+, so `1e`,
    // `1e+`, `5e-`, and friends must fail at lex time (previously `1e` split
    // into Integer(1) + Identifier(e) and `5e-` produced a Float("5e-") token
    // that only failed later inside `f64` parsing).
    for (source, expected_span) in [
        ("1e", Span::new(0, 2)),
        ("1e+", Span::new(0, 3)),
        ("1e-", Span::new(0, 3)),
        ("5e-", Span::new(0, 3)),
        ("5e+", Span::new(0, 3)),
        ("-1e", Span::new(0, 3)),
        ("1.5e", Span::new(0, 4)),
        ("1.5e-", Span::new(0, 5)),
        ("1e 5", Span::new(0, 2)),
    ] {
        let mut lexer = Lexer::new(source);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(error.code, "QQL-LEX-NUMBER", "source: {source}");
        assert_eq!(error.span, Some(expected_span), "source: {source}");
    }
}

#[test]
fn trailing_dot_is_a_structured_lex_error() {
    // grammar `float` requires digits after the decimal point, so `1.` (and a
    // dot following an exponent such as `1e5.3`) is malformed at lex time.
    for (source, expected_span) in [
        ("1.", Span::new(0, 2)),
        ("1. ", Span::new(0, 2)),
        ("3.14.", Span::new(0, 5)),
        ("1e5.3", Span::new(0, 4)),
        ("-1.", Span::new(0, 3)),
    ] {
        let mut lexer = Lexer::new(source);
        let error = lexer.next().unwrap().unwrap_err();
        assert_eq!(error.code, "QQL-LEX-NUMBER", "source: {source}");
        assert_eq!(error.span, Some(expected_span), "source: {source}");
    }
}

#[test]
fn minus_before_dot_is_a_structured_lex_error() {
    // `-.5` is not a number per grammar (`-` requires digits); error at lex
    // time rather than emitting `-` plus a bare-dot QQL-LEX-CHAR.
    let mut lexer = Lexer::new("-.5");
    let error = lexer.next().unwrap().unwrap_err();
    assert_eq!(error.code, "QQL-LEX-NUMBER");
    assert_eq!(error.span, Some(Span::new(0, 2)));

    // A standalone dot after whitespace is still an unexpected character.
    let mut lexer = Lexer::new(".5");
    let error = lexer.next().unwrap().unwrap_err();
    assert_eq!(error.code, "QQL-LEX-CHAR");
    assert_eq!(error.span, Some(Span::new(0, 1)));

    // `-` followed by anything other than a digit/dot stays the minus operator.
    let mut lexer = Lexer::new("x - .5");
    let first = lexer.next().unwrap().unwrap();
    assert_eq!(first.kind, TokenKind::Identifier);
    let second = lexer.next().unwrap().unwrap();
    assert_eq!(second.kind, TokenKind::Minus);
    assert!(lexer.next().unwrap().is_err());
}
