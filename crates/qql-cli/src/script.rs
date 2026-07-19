use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_core::token::TokenKind;

pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    let mut quote_char: u8 = 0;

    while i < bytes.len() {
        let ch = bytes[i];

        if in_string {
            push_input_char(&mut out, text, &mut i);
            if ch == b'\\' && i < bytes.len() {
                push_input_char(&mut out, text, &mut i);
                continue;
            }
            if ch == quote_char {
                in_string = false;
                quote_char = 0;
            }
            continue;
        }

        if ch == b'\'' || ch == b'"' {
            in_string = true;
            quote_char = ch;
            push_input_char(&mut out, text, &mut i);
            continue;
        }

        if ch == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\r' && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        push_input_char(&mut out, text, &mut i);
    }

    out
}

fn push_input_char(output: &mut String, input: &str, index: &mut usize) {
    let ch = input[*index..]
        .chars()
        .next()
        .expect("index is always within the input while copying a character");
    output.push(ch);
    *index += ch.len_utf8();
}

pub fn split_statements(text: &str) -> Result<Vec<String>, QqlError> {
    let cleaned = strip_comments(text);
    let mut lexer = Lexer::new(&cleaned);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token()?;
        if tok.kind == TokenKind::Eof {
            break;
        }
        tokens.push(tok);
    }

    let mut depth: i32 = 0;
    let mut statement_start = 0;
    let mut statements = Vec::new();
    for tok in &tokens {
        match tok.kind {
            TokenKind::Lbrace | TokenKind::Lbracket | TokenKind::Lparen => depth += 1,
            TokenKind::Rbrace | TokenKind::Rbracket | TokenKind::Rparen => {
                depth -= 1;
                if depth < 0 {
                    return Err(QqlError::syntax(
                        format!(
                            "unexpected '{}' at position {} (unmatched closing delimiter)",
                            tok.text, tok.pos
                        ),
                        tok.pos,
                    ));
                }
            }
            TokenKind::Semicolon if depth == 0 => {
                push_statement(&cleaned, statement_start, tok.pos, &mut statements)?;
                statement_start = tok.pos + tok.text.len();
            }
            _ => {}
        }
    }
    if depth > 0 {
        return Err(QqlError::syntax(
            format!("unexpected end of input: {} unclosed delimiter(s)", depth),
            0,
        ));
    }

    push_statement(&cleaned, statement_start, cleaned.len(), &mut statements)?;

    Ok(statements)
}

fn push_statement(
    input: &str,
    start: usize,
    end: usize,
    statements: &mut Vec<String>,
) -> Result<(), QqlError> {
    let statement = input[start..end].trim();
    if !statement.is_empty() {
        Parser::parse(statement)?;
        statements.push(statement.to_string());
    }
    Ok(())
}

pub fn read_script(path: &str) -> Result<Vec<String>, QqlError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| QqlError::runtime(format!("cannot read file: {}", e)))?;
    split_statements(&data)
}

pub fn execute_script<F>(lines: Vec<String>, stop: bool, mut exec: F) -> Result<(u32, u32), String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;
    for stmt in &lines {
        match exec(stmt) {
            Ok(_) => ok_count += 1,
            Err(e) => {
                fail_count += 1;
                if stop {
                    return Err(format!(
                        "error at statement {}: {}",
                        ok_count + fail_count,
                        e
                    ));
                }
            }
        }
    }
    Ok((ok_count, fail_count))
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
    fn rejects_adjacent_statements_without_a_semicolon() {
        let result = split_statements("SHOW COLLECTIONS SHOW COLLECTION docs");

        assert!(result.is_err());
    }
}
