/// Typed filter expressions (`WHERE` predicates and comparisons).
pub mod filter;
/// Scoring formula expressions (`FORMULA` arithmetic and decay).
pub mod formula;
/// Typed AST for QQL statements (`Stmt` and its variants).
pub mod statement;
/// AST transforms: filter injection and shard-key routing.
pub mod transform;
/// Literal `Value`s for payloads, filters, and collection configs.
pub mod value;

pub use filter::{ComparisonOp, FilterExpr, GeoPoint, PointIdPredicate};
pub use formula::{FormulaExpr, looks_like_iso_datetime};
pub use statement::*;
pub use transform::inject_filter;
pub use value::Value;

/// Check if a string is a simple QQL identifier (`[a-zA-Z_][a-zA-Z0-9_]*`).
pub fn is_simple_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Escape a string for a QQL single-quoted literal without enclosing quotes.
///
/// Only escape sequences the parser decodes are emitted (`\\`, `\'`, `\n`,
/// `\r`, `\t`), so the rendered literal always re-parses to the same content.
/// Null bytes are dropped as the parser has no escape for them.
pub fn escape_string(value: &str) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => {}
            c => out.push(c),
        }
    }
    out
}
