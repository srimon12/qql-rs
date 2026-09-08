//! Table cell primitives, text alignment, and sanitization.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Text alignment within a table cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    Left,
    Right,
}

/// A formatted cell with an explicit text value and horizontal alignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cell {
    pub value: String,
    pub alignment: Alignment,
}

impl Cell {
    /// Create a left-aligned cell with sanitized control characters.
    pub fn text(value: impl Into<String>) -> Self {
        Self {
            value: escape_controls(&value.into()),
            alignment: Alignment::Left,
        }
    }

    /// Create a cell inferred from a JSON value.
    ///
    /// Numbers are automatically right-aligned; all other types are left-aligned.
    pub fn from_json(value: Option<&serde_json::Value>) -> Self {
        let alignment = match value {
            Some(serde_json::Value::Number(_)) => Alignment::Right,
            _ => Alignment::Left,
        };

        let mut cell = Self::text(value.map(stringify_value).unwrap_or_default());
        cell.alignment = alignment;
        cell
    }
}

/// Returns the terminal-cell width according to Unicode Standard Annex #11.
pub fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

/// Escapes control characters so a value cannot break table structure or emit
/// terminal control sequences. Printable Unicode remains unchanged.
pub fn escape_controls(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

/// Converts a JSON value into its canonical tabular string representation.
pub fn stringify_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}

/// Truncates a string to at most `max_width` terminal columns, appending `…` if truncated.
/// Respects UTF-8 character boundaries and multi-column Unicode widths.
pub fn truncate_width(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(s) <= max_width {
        return s.to_string();
    }
    let target = max_width.saturating_sub(1);
    let mut accumulated = 0;
    let mut end_byte = 0;
    for (idx, ch) in s.char_indices() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if accumulated + w > target {
            break;
        }
        accumulated += w;
        end_byte = idx + ch.len_utf8();
    }
    let mut res = s[..end_byte].to_string();
    res.push('…');
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_from_json_alignments() {
        let num_cell = Cell::from_json(Some(&serde_json::json!(42)));
        assert_eq!(num_cell.alignment, Alignment::Right);
        assert_eq!(num_cell.value, "42");

        let str_cell = Cell::from_json(Some(&serde_json::json!("hello")));
        assert_eq!(str_cell.alignment, Alignment::Left);
        assert_eq!(str_cell.value, "hello");
    }

    #[test]
    fn test_truncate_width() {
        assert_eq!(truncate_width("hello world", 7), "hello …");
        assert_eq!(truncate_width("hello", 10), "hello");
        assert_eq!(truncate_width("東京tokyo", 5), "東京…");
    }
}
