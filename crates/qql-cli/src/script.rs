use qql_core::error::{QqlError, Span};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_core::token::{Token, TokenKind};

fn is_contextual_identifier(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Offset
            | TokenKind::Score
            | TokenKind::Threshold
            | TokenKind::Lookup
            | TokenKind::Id
            | TokenKind::Dense
            | TokenKind::Sparse
            | TokenKind::Vector
    )
}

/// Whether the token at `index` opens a top-level statement.
///
/// Uses `qql-core`'s authoritative statement table plus `BATCH` (which
/// `parse_stmt` accepts but the core table omits for recovery purposes).
/// `WITH` only opens a statement when it is a CTE header (`WITH name AS (…));
/// the `WITH PAYLOAD` / `WITH PARAMS` clause forms are not statement starts.
fn is_statement_opener(tokens: &[Token<'_>], index: usize) -> bool {
    let kind = tokens[index].kind;
    if kind == TokenKind::With {
        if index + 2 >= tokens.len() {
            return false;
        }
        let next1 = &tokens[index + 1];
        let next2 = &tokens[index + 2];
        let next1_is_ident = next1.kind == TokenKind::Identifier
            || next1.kind == TokenKind::String
            || is_contextual_identifier(next1.kind);
        return next1_is_ident && next2.kind == TokenKind::As;
    }
    kind.is_statement_start() || kind == TokenKind::Batch
}

/// Lex the raw script, keeping every token (comments are skipped by the lexer,
/// so string literals — including backtick, raw, and triple-quoted forms —
/// stay intact).
fn lex_tokens(text: &str) -> Result<Vec<Token<'_>>, QqlError> {
    let mut lexer = Lexer::new(text);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token()?;
        if tok.kind == TokenKind::Eof {
            break;
        }
        tokens.push(tok);
    }
    Ok(tokens)
}

/// Split a script on top-level statement boundaries.
///
/// Boundaries are token spans from the `qql-core` lexer — the same statement
/// table the parser uses — so comments cannot split a statement and comment
/// text inside string literals is preserved verbatim. Each returned statement
/// is trimmed of its trailing `;` and validated with [`Parser::parse`].
pub fn split_statements(text: &str) -> Result<Vec<String>, QqlError> {
    let tokens = lex_tokens(text)?;

    let mut starts: Vec<usize> = Vec::new();
    let mut depth: i32 = 0;
    let mut in_with_cte = false;
    for (i, tok) in tokens.iter().enumerate() {
        // After a WITH ... AS (...) CTE block, suppress the next QUERY/FUSION
        // from being treated as a new statement starter — it is the main query
        // of the WITH statement.
        if in_with_cte && depth == 0 && matches!(tok.kind, TokenKind::Query | TokenKind::Fusion) {
            in_with_cte = false;
        } else if depth == 0 && is_statement_opener(&tokens, i) {
            starts.push(i);
            if tok.kind == TokenKind::With {
                in_with_cte = true;
            }
        }

        match tok.kind {
            TokenKind::Lbrace | TokenKind::Lbracket | TokenKind::Lparen => depth += 1,
            TokenKind::Rbrace | TokenKind::Rbracket | TokenKind::Rparen => {
                depth -= 1;
                if depth < 0 {
                    return Err(QqlError::parse(
                        "QQL-PARSE-DELIMITER",
                        format!(
                            "unexpected '{}' at position {} (unmatched closing delimiter)",
                            tok.text, tok.span.start,
                        ),
                        tok.span,
                    ));
                }
            }
            _ => {}
        }
    }
    if depth > 0 {
        return Err(QqlError::parse(
            "QQL-PARSE-DELIMITER",
            format!("unexpected end of input: {} unclosed delimiter(s)", depth),
            Span::new(0, text.len()),
        ));
    }

    if starts.is_empty() {
        return Ok(Vec::new());
    }

    let mut statements = Vec::new();
    for (idx, &start) in starts.iter().enumerate() {
        let end_idx = starts.get(idx + 1).copied().unwrap_or(tokens.len());
        // The statement's source text runs to its last non-separator token, so
        // trailing comments and the `;` separator are excluded.
        let last = tokens[start..end_idx]
            .iter()
            .rev()
            .find(|tok| tok.kind != TokenKind::Semicolon);
        let Some(last) = last else { continue };
        let stmt = text[tokens[start].span.start..last.span.end].trim();

        if !stmt.is_empty() {
            Parser::parse(stmt)?;
            statements.push(stmt.to_string());
        }
    }

    Ok(statements)
}

pub fn read_script(path: &str) -> Result<Vec<String>, QqlError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| QqlError::execution("QQL-CLI-IO", format!("cannot read file: {e}"), None))?;
    split_statements(&data)
}

#[cfg(test)]
mod tests {
    use super::split_statements;

    #[test]
    fn splits_top_level_semicolons_without_breaking_ctes() {
        let script = "WITH dense AS (QUERY 'search' LIMIT 10) QUERY 'search' FROM docs PREFETCH (dense); SHOW COLLECTIONS;";

        let statements = split_statements(script).expect("script should parse");

        assert_eq!(statements.len(), 2);
        assert!(statements[0].starts_with("WITH dense"));
        assert_eq!(statements[1], "SHOW COLLECTIONS");
    }

    #[test]
    fn preserves_unicode_string_literals() {
        let statements =
            split_statements("QUERY 'café' FROM docs LIMIT 1;").expect("script should parse");

        assert_eq!(statements, ["QUERY 'café' FROM docs LIMIT 1"]);
    }

    #[test]
    fn splits_every_statement_starter() {
        // Regression: these five starters were missing from the hand-rolled
        // table, so a script led by them silently executed nothing.
        let cases = [
            "COUNT FROM docs",
            "FACET city FROM docs",
            "CLEAR PAYLOAD FROM docs WHERE id = 1",
            "SET QUOTA (enabled = true)",
            "BATCH { QUERY [0.1] FROM docs LIMIT 1; }",
        ];
        for case in cases {
            let script = format!("{case};");
            let statements = split_statements(&script)
                .unwrap_or_else(|e| panic!("'{case}' should split: {e}"));
            assert_eq!(statements, [case], "starter lost for '{case}'");
        }
    }

    #[test]
    fn splits_a_script_led_by_each_starter() {
        for leader in [
            "COUNT FROM docs",
            "FACET city FROM docs",
            "CLEAR PAYLOAD FROM docs WHERE id = 1",
            "SET QUOTA (enabled = true)",
            "BATCH { QUERY [0.1] FROM docs LIMIT 1; }",
        ] {
            let script = format!("{leader};\nSHOW COLLECTIONS;");
            let statements =
                split_statements(&script).unwrap_or_else(|e| panic!("'{leader}' failed: {e}"));
            assert_eq!(statements.len(), 2, "wrong split for '{leader}'");
            assert_eq!(statements[0], leader);
            assert_eq!(statements[1], "SHOW COLLECTIONS");
        }
    }

    #[test]
    fn comments_and_string_literals_survive_splitting() {
        // `--` inside backtick, raw, and triple-quoted strings is content,
        // not a comment; it must survive byte-identically.
        let script = concat!(
            "UPSERT INTO docs VALUES {id: 1, a: `x--y`};\n",
            "-- a real comment\n",
            "UPSERT INTO docs VALUES {id: 2, b: r'C:\\x--y'};\n",
            "QUERY '''line--one''' FROM docs;\n",
        );
        let statements = split_statements(script).expect("script should parse");
        assert_eq!(statements.len(), 3);
        assert!(statements[0].contains("`x--y`"));
        assert!(statements[1].contains(r"r'C:\x--y'"), "got: {}", statements[1]);
        assert!(statements[2].contains("'''line--one'''"));
    }
}
