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

    let mut starts = Vec::new();
    let mut depth: i32 = 0;
    for (i, tok) in tokens.iter().enumerate() {
        let is_starter = match tok.kind {
            TokenKind::Insert
                | TokenKind::Create
                | TokenKind::Alter
                | TokenKind::Drop
                | TokenKind::Show
                | TokenKind::Query
                | TokenKind::Select
                | TokenKind::Scroll
                | TokenKind::Delete
                | TokenKind::Update => true,
            TokenKind::With => {
                if i + 2 < tokens.len() {
                    let next1 = &tokens[i + 1];
                    let next2 = &tokens[i + 2];
                    let next1_is_ident = next1.kind == TokenKind::Identifier
                        || next1.kind == TokenKind::String
                        || is_contextual_identifier(next1.kind);
                    next1_is_ident && next2.kind == TokenKind::As
                } else {
                    false
                }
            }
            _ => false,
        };

        if depth == 0 && is_starter {
            starts.push(tok.pos);
        }

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
            _ => {}
        }
    }
    if depth > 0 {
        return Err(QqlError::syntax(
            format!("unexpected end of input: {} unclosed delimiter(s)", depth),
            0,
        ));
    }

    if starts.is_empty() {
        return Ok(Vec::new());
    }

    let mut statements = Vec::new();
    for (idx, &start) in starts.iter().enumerate() {
        let end = if idx + 1 < starts.len() {
            starts[idx + 1]
        } else {
            cleaned.len()
        };

        let mut stmt = cleaned[start..end].trim();
        if stmt.ends_with(';') {
            stmt = stmt[..stmt.len() - 1].trim();
        }

        if !stmt.is_empty() {
            Parser::parse(stmt)?;
            statements.push(stmt.to_string());
        }
    }

    Ok(statements)
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
}
