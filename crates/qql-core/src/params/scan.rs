//! Lexical scanner helpers for parameter placeholders and protected spans.

/// Returns true if the `:` at byte offset `i` is at a valid token boundary to begin a parameter placeholder.
pub fn is_placeholder_start(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    !matches!(
        bytes[i - 1],
        b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
            | b'_'
            | b'$'
            | b'\''
            | b'"'
            | b'`'
            | b'}'
            | b']'
    )
}

/// Check if a byte is a valid parameter identifier start character.
#[inline]
pub fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Check if a byte is a valid parameter identifier continue character.
#[inline]
pub fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Advance past a `--` line comment. `i` is the first `-`. Stops before `\n`.
pub fn scan_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Advance past a backtick-quoted span including the closing `` ` `` if present.
pub fn scan_backtick(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Advance past a single-quoted literal, honoring `''` and `\` escapes.
pub fn scan_single_quoted(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        let sc = bytes[i];
        i += 1;
        if sc == b'\\' && i < bytes.len() {
            i += 1;
        } else if sc == b'\'' {
            if i < bytes.len() && bytes[i] == b'\'' {
                i += 1;
            } else {
                break;
            }
        }
    }
    i
}

#[inline]
fn has_closing_triple(bytes: &[u8], start: usize, triple: &[u8; 3]) -> bool {
    start + 2 < bytes.len() && &bytes[start..start + 3] == triple
}

/// Advance past a triple-quoted string (`'''...'''` or `"""..."""`).
pub fn scan_triple_quoted(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    i += 3;
    let triple = [quote, quote, quote];
    while i < bytes.len() {
        if has_closing_triple(bytes, i, &triple) {
            return i + 3;
        }
        i += 1;
    }
    i
}

/// Advance past a raw string literal `r'...'` or `r"..."`.
pub fn scan_raw_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    i += 2;
    while i < bytes.len() {
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Advance past a double-quoted string literal (which may contain `\"`).
pub fn scan_double_quoted(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        let sc = bytes[i];
        i += 1;
        if sc == b'\\' && i < bytes.len() {
            i += 1;
        } else if sc == b'"' {
            break;
        }
    }
    i
}

/// If `bytes[i]` begins a comment, string literal, or backtick identifier,
/// returns `Some(next_offset)` skipping the protected region. Otherwise `None`.
pub fn skip_protected(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes[i] {
        b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
            Some(scan_line_comment(bytes, i + 2))
        }
        b'`' => Some(scan_backtick(bytes, i)),
        b'\'' => {
            if i + 2 < bytes.len() && bytes[i + 1] == b'\'' && bytes[i + 2] == b'\'' {
                Some(scan_triple_quoted(bytes, i, b'\''))
            } else {
                Some(scan_single_quoted(bytes, i))
            }
        }
        b'"' => {
            if i + 2 < bytes.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                Some(scan_triple_quoted(bytes, i, b'"'))
            } else {
                Some(scan_double_quoted(bytes, i))
            }
        }
        b'r' if i + 1 < bytes.len() && (bytes[i + 1] == b'\'' || bytes[i + 1] == b'"') => {
            let quote = bytes[i + 1];
            if i + 3 < bytes.len() && bytes[i + 2] == quote && bytes[i + 3] == quote {
                Some(scan_triple_quoted(bytes, i + 1, quote))
            } else {
                Some(scan_raw_string(bytes, i, quote))
            }
        }
        _ => None,
    }
}

/// ASCII identifier slice at `[start, end)`. Infallible because the scanner
/// only advances over `is_ident_start` / `is_ident_continue` bytes.
pub fn ident_at(source: &str, start: usize, end: usize) -> &str {
    source.get(start..end).unwrap_or("")
}
