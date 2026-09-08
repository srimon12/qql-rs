//! Identifier and string literal escaping for QQL script generation.

/// Escape a string for QQL single-quoted literals.
///
/// Only sequences recognized by `qql-core`'s `decode_string` are emitted:
/// `\\`, `\'`, `\n`, `\r`, `\t`. Null bytes and other control chars are
/// stripped so the dump remains round-trippable via `qql execute`.
pub fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => {} // unsupported by qql-core — drop rather than emit \0
            c => out.push(c),
        }
    }
    out
}

/// Checks whether an identifier can remain unquoted in QQL syntax.
pub fn is_simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Format a collection or field name for QQL. Simple idents stay bare; others are quoted.
pub fn format_ident(name: &str) -> String {
    if is_simple_ident(name) {
        name.to_string()
    } else {
        format!("'{}'", escape_string(name))
    }
}
