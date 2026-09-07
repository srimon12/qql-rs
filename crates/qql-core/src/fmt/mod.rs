//! Canonical QQL formatter.
//!
//! Parses source text into the typed AST and re-emits it in canonical form.
//! The output is normalized (canonical clause order, canonical keyword casing,
//! escaped string literals) while preserving comments, file headers, and
//! blank-line trivia, and always re-parses to an identical AST:
//!
//! ```text
//! parse(format(parse(input))) == parse(input)
//! format(format(input))        == format(input)
//! ```

pub(crate) mod ddl;
pub(crate) mod expr;
pub(crate) mod filter;
pub(crate) mod formula;
pub(crate) mod index_quota;
pub(crate) mod mutation;
pub(crate) mod query;
pub(crate) mod query_expr;

#[cfg(test)]
mod tests;

pub use expr::render_filter;
pub(crate) use query::render_search_params;

use crate::ast::Stmt;
use crate::error::QqlError;
use crate::parser::Parser;
use alloc::string::String;
use alloc::vec::Vec;

/// Default maximum dimensions shown in compact vector previews before `... (N dims)`.
pub const DEFAULT_PREVIEW_MAX_DIMS: usize = 2;

/// Parse `source` and render it in canonical QQL form, preserving comments and blank lines.
pub fn format(source: &str) -> Result<String, QqlError> {
    let statements_with_spans = Parser::parse_all_with_spans(source)?;
    if statements_with_spans.is_empty() {
        let (comments, _) = parse_trivia_lines_with_trailing_blank(source);
        if comments.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::new();
        for line in comments {
            out.push_str(&line);
            out.push('\n');
        }
        return Ok(out);
    }

    let mut out = String::new();
    let n = statements_with_spans.len();

    // 1. File header before the first statement
    let first_stmt_start = statements_with_spans[0].1.start;
    let header_slice = &source[..first_stmt_start];
    let (header_comments, had_blank_after_header) =
        parse_trivia_lines_with_trailing_blank(header_slice);
    if !header_comments.is_empty() {
        for line in &header_comments {
            out.push_str(line);
            out.push('\n');
        }
        if had_blank_after_header {
            out.push('\n');
        }
    }

    // 2. Format each statement and following trivia
    for i in 0..n {
        let (stmt, stmt_span) = &statements_with_spans[i];

        // Check if there are any inline comments inside the statement body
        let inner_slice = &source[stmt_span.start..stmt_span.end];
        let inline_comments = find_comments_in_slice(inner_slice);
        for comment in inline_comments {
            out.push_str(comment);
            out.push('\n');
        }

        // Render the statement
        out.push_str(&format_stmt(stmt));
        out.push(';');

        // Check gap following statement
        let gap_start = stmt_span.end;
        let gap_end = if i + 1 < n {
            statements_with_spans[i + 1].1.start
        } else {
            source.len()
        };
        let gap_slice = &source[gap_start..gap_end];

        let (same_line, rest) = match gap_slice.find('\n') {
            Some(idx) => (&gap_slice[..idx], &gap_slice[idx + 1..]),
            None => (gap_slice, ""),
        };

        // Trailing comment on the same line after semicolon
        if let Some(pos) = same_line.find("--") {
            out.push(' ');
            out.push_str(same_line[pos..].trim_end());
        }
        out.push('\n');

        if i + 1 < n {
            // Inter-statement gap
            let (gap_comments, had_blank_after_gap) = parse_trivia_lines_with_trailing_blank(rest);
            if gap_comments.is_empty() {
                // No comments: check if there was a blank line between statements
                if gap_slice.matches('\n').count() >= 2 {
                    out.push('\n');
                }
            } else {
                // Comments between statements: precede with a blank line for readability
                out.push('\n');
                for line in &gap_comments {
                    out.push_str(line);
                    out.push('\n');
                }
                if had_blank_after_gap {
                    out.push('\n');
                }
            }
        } else {
            // Trailing trivia after last statement
            let (trailer_comments, _) = parse_trivia_lines_with_trailing_blank(rest);
            if !trailer_comments.is_empty() {
                out.push('\n');
                for line in &trailer_comments {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }

    Ok(out)
}

fn parse_trivia_lines_with_trailing_blank(slice: &str) -> (Vec<String>, bool) {
    let mut comments = Vec::new();
    let mut pending_blank = false;

    let had_blank_after = if let Some(last_comment_idx) = slice.rfind("--") {
        let after_comment = &slice[last_comment_idx..];
        if let Some(nl_idx) = after_comment.find('\n') {
            after_comment[nl_idx + 1..].contains('\n')
        } else {
            false
        }
    } else {
        false
    };

    for line in slice.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !comments.is_empty() {
                pending_blank = true;
            }
        } else if trimmed.starts_with("--") {
            if pending_blank {
                comments.push(String::new());
                pending_blank = false;
            }
            comments.push(trimmed.into());
        }
    }

    (comments, had_blank_after)
}

fn find_comments_in_slice(slice: &str) -> Vec<&str> {
    let mut comments = Vec::new();
    let bytes = slice.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[pos];
                pos += 1;
                let is_triple =
                    pos + 1 < bytes.len() && bytes[pos] == quote && bytes[pos + 1] == quote;
                if is_triple {
                    pos += 2;
                    let delim = if quote == b'\'' { "'''" } else { "\"\"\"" };
                    if let Some(idx) = slice[pos..].find(delim) {
                        pos += idx + 3;
                    } else {
                        break;
                    }
                } else {
                    while pos < bytes.len() {
                        if bytes[pos] == b'\\' {
                            pos += 2;
                            continue;
                        }
                        if bytes[pos] == quote {
                            if quote == b'\'' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\'' {
                                pos += 2;
                                continue;
                            }
                            pos += 1;
                            break;
                        }
                        pos += 1;
                    }
                }
            }
            b'-' if pos + 1 < bytes.len() && bytes[pos + 1] == b'-' => {
                let start = pos;
                pos += 2;
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
                comments.push(slice[start..pos].trim_end());
            }
            _ => {
                pos += 1;
            }
        }
    }
    comments
}

/// Render a list of statements as a canonical script (each terminated by `;`,
/// joined by newlines).
pub fn format_script(statements: &[Stmt]) -> String {
    let mut out = String::new();
    for (i, statement) in statements.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_stmt(statement));
        out.push(';');
    }
    out
}

/// Render a single statement in canonical QQL form (no trailing `;`).
pub fn format_stmt(statement: &Stmt) -> String {
    match statement {
        Stmt::Query(query) => query::render_query_body(query),
        Stmt::Scroll(statement) => mutation::render_scroll(statement),
        Stmt::Upsert(statement) => mutation::render_upsert(statement),
        Stmt::CreateCollection(statement) => ddl::render_create_collection(statement),
        Stmt::CreateIndex(statement) => ddl::render_create_index(statement),
        Stmt::DropIndex(statement) => ddl::render_drop_index(statement),
        Stmt::CreateShardKey(statement) => ddl::render_create_shard_key(statement),
        Stmt::DropShardKey(statement) => ddl::render_drop_shard_key(statement),
        Stmt::AlterCollection(statement) => ddl::render_alter_collection(statement),
        Stmt::DropCollection(statement) => ddl::render_drop_collection(statement),
        Stmt::ShowCollections => ddl::render_show_collections(),
        Stmt::ShowCollection(collection) => ddl::render_show_collection(collection),
        Stmt::ShowShardKeys(collection) => ddl::render_show_shard_keys(collection),
        Stmt::ShowQuotas => ddl::render_show_quotas(),
        Stmt::SetQuota(stmt) => ddl::render_set_quota(stmt),
        Stmt::Delete(statement) => mutation::render_delete(statement),
        Stmt::ClearPayload(statement) => mutation::render_clear_payload(statement),
        Stmt::DeletePayload(statement) => mutation::render_delete_payload(statement),
        Stmt::DeleteVector(statement) => mutation::render_delete_vector(statement),
        Stmt::UpdateVector(statement) => mutation::render_update_vector(statement),
        Stmt::UpdatePayload(statement) => mutation::render_update_payload(statement),
        Stmt::Count(statement) => mutation::render_count(statement),
        Stmt::Facet(statement) => mutation::render_facet(statement),
    }
}

/// Render a single statement in human-readable QQL form with compact vector literals.
pub fn format_stmt_readable(statement: &Stmt) -> String {
    crate::params::truncate_vector_literals(&format_stmt(statement), DEFAULT_PREVIEW_MAX_DIMS)
}
