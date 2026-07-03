use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
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
            out.push(ch as char);
            if ch == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if ch == quote_char {
                in_string = false;
                quote_char = 0;
            }
            i += 1;
            continue;
        }

        if ch == b'\'' || ch == b'"' {
            in_string = true;
            quote_char = ch;
            out.push(ch as char);
            i += 1;
            continue;
        }

        if ch == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\r' && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        out.push(ch as char);
        i += 1;
    }

    out
}

fn is_statement_starter(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Insert
            | TokenKind::Create
            | TokenKind::Alter
            | TokenKind::Drop
            | TokenKind::Show
            | TokenKind::Query
            | TokenKind::Select
            | TokenKind::Scroll
            | TokenKind::Delete
            | TokenKind::Update
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

    let mut starts: Vec<usize> = Vec::new();
    let mut depth: i32 = 0;
    for tok in &tokens {
        if depth == 0 && is_statement_starter(tok.kind) {
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

    let mut statements = Vec::with_capacity(starts.len());
    for i in 0..starts.len() {
        let start = starts[i];
        let end = if i + 1 < starts.len() {
            starts[i + 1]
        } else {
            cleaned.len()
        };
        let stmt = cleaned[start..end].trim();
        if !stmt.is_empty() {
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
