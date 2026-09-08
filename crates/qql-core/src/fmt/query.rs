//! Formatting for QUERY statements.

pub(crate) use super::query_expr::{
    query_expr_prefetch, query_expr_using, render_prefetch, render_query_expr,
    render_search_params, render_vector_target,
};

use crate::ast::{QueryCollection, QueryStmt};
use crate::fmt::expr::{
    render_f64, render_name, render_payload_selector, render_placeholder, render_vector_selector,
};
use crate::fmt::filter::render_filter;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

pub(crate) fn render_query_body(query: &QueryStmt) -> String {
    let mut out = String::new();
    let has_ctes = !query.ctes.is_empty();
    if has_ctes {
        out.push_str("WITH\n");
        for (i, cte) in query.ctes.iter().enumerate() {
            if i > 0 {
                out.push_str(",\n");
            }
            let _ = write!(out, "  {} AS (", render_name(&cte.name));
            out.push_str(&render_query_body_inner(&cte.query));
            out.push(')');
        }
        out.push('\n');
    }
    out.push_str(&render_query_body_formatted(query, has_ctes));
    out
}

pub(crate) fn query_tail_clauses(query: &QueryStmt) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(using) = query_expr_using(&query.expression) {
        parts.push(format!("USING {}", render_vector_target(using)));
    }
    let prefetch = query_expr_prefetch(&query.expression);
    if !prefetch.is_empty() {
        parts.push(format!(
            "PREFETCH ({})",
            prefetch
                .iter()
                .map(render_prefetch)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(filter) = &query.filter {
        parts.push(format!("WHERE {}", render_filter(filter)));
    }
    if let Some(key) = &query.shard_key {
        parts.push(format!("SHARD {key}"));
    }
    if let Some(params) = &query.params {
        parts.push(format!("PARAMS ({})", render_search_params(params)));
    }
    if let Some(score) = query.score_threshold {
        parts.push(format!("SCORE THRESHOLD {}", render_f64(score)));
    }
    if let Some(group) = &query.group {
        let mut clause = format!("GROUP BY {}", render_name(&group.field));
        if let Some(size) = group.size {
            let _ = write!(clause, " SIZE {}", size);
        }
        if let Some(lookup) = &group.lookup {
            let _ = write!(clause, " LOOKUP FROM {}", render_name(lookup));
        }
        parts.push(clause);
    }
    if let Some(selector) = &query.output.payload {
        parts.push(format!(
            "WITH PAYLOAD {}",
            render_payload_selector(selector)
        ));
    }
    if let Some(selector) = &query.output.vectors {
        parts.push(format!("WITH VECTOR {}", render_vector_selector(selector)));
    }
    if let Some(limit) = query.page.limit {
        parts.push(format!("LIMIT {}", limit));
    } else if let Some(param) = &query.page.limit_param {
        parts.push(format!("LIMIT {}", render_placeholder(param)));
    }
    if let Some(offset) = query.page.offset {
        parts.push(format!("OFFSET {}", offset));
    } else if let Some(param) = &query.page.offset_param {
        parts.push(format!("OFFSET {}", render_placeholder(param)));
    }
    parts
}

/// Render `QUERY <expr> [FROM c] <tail>` formatted with standard single-line/multiline layout.
pub(crate) fn render_query_body_formatted(query: &QueryStmt, has_ctes: bool) -> String {
    let expr_str = format!("QUERY {}", render_query_expr(&query.expression));
    let coll_str = match &query.collection {
        QueryCollection::Explicit(collection) => Some(format!("FROM {}", render_name(collection))),
        QueryCollection::Inherited => None,
    };
    let tail_clauses = query_tail_clauses(query);

    let single_line_len = expr_str.len()
        + coll_str.as_ref().map(|s| s.len() + 1).unwrap_or(0)
        + tail_clauses.iter().map(|s| s.len() + 1).sum::<usize>();

    let is_multiline = has_ctes
        || tail_clauses.len() >= 3
        || single_line_len > 80
        || tail_clauses
            .iter()
            .any(|c| c.starts_with("PREFETCH") && c.contains(','));

    if is_multiline {
        let mut lines = Vec::new();
        lines.push(expr_str);
        if let Some(coll) = coll_str {
            lines.push(coll);
        }
        for clause in tail_clauses {
            lines.push(clause);
        }
        lines.join("\n")
    } else {
        let mut parts = Vec::new();
        parts.push(expr_str);
        if let Some(coll) = coll_str {
            parts.push(coll);
        }
        for clause in tail_clauses {
            parts.push(clause);
        }
        parts.join(" ")
    }
}

/// Render `QUERY <expr> [FROM c] <tail>` on a single line. Used for
/// CTE bodies and inline prefetch queries, which cannot declare their own CTEs.
pub(crate) fn render_query_body_inner(query: &QueryStmt) -> String {
    let mut out = String::from("QUERY ");
    out.push_str(&render_query_expr(&query.expression));
    if let QueryCollection::Explicit(collection) = &query.collection {
        let _ = write!(out, " FROM {}", render_name(collection));
    }
    let tail_clauses = query_tail_clauses(query);
    if !tail_clauses.is_empty() {
        out.push(' ');
        out.push_str(&tail_clauses.join(" "));
    }
    out
}
