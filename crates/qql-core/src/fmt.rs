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

use crate::ast::*;
use crate::error::QqlError;
use crate::parser::Parser;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

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
        Stmt::Query(query) => render_query_body(query),
        Stmt::Scroll(statement) => {
            let mut out = format!("SCROLL FROM {}", render_name(&statement.collection));
            if let Some(filter) = &statement.filter {
                let _ = write!(out, " WHERE {}", render_filter(filter));
            }
            if let Some(after) = &statement.after {
                let _ = write!(out, " AFTER {}", render_point_id(after));
            }
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            if let Some(selector) = &statement.with_vector {
                let _ = write!(out, " WITH VECTOR {}", render_vector_selector(selector));
            }
            let _ = write!(out, " LIMIT {}", statement.limit);
            out
        }
        Stmt::Upsert(statement) => {
            let mut out = format!("UPSERT INTO {} VALUES", render_name(&statement.collection));
            let multiline_points = statement.points.len() > 1;
            for (i, point) in statement.points.iter().enumerate() {
                if multiline_points {
                    if i == 0 {
                        out.push_str("\n  ");
                    } else {
                        out.push_str(",\n  ");
                    }
                } else if i > 0 {
                    out.push_str(", ");
                } else {
                    out.push(' ');
                }
                out.push_str(&render_point(point));
            }
            if let Some(embedding) = &statement.embedding {
                let _ = write!(out, " USING {}", render_embedding_spec(embedding));
            }
            if !statement.embed.is_empty() {
                if statement.embed.len() > 1 {
                    out.push_str("\nEMBED ");
                    for (i, directive) in statement.embed.iter().enumerate() {
                        if i > 0 {
                            out.push_str(",\n  ");
                        }
                        out.push_str(&render_embed_directive(directive));
                    }
                } else {
                    out.push_str(" EMBED ");
                    out.push_str(&render_embed_directive(&statement.embed[0]));
                }
            }
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::CreateCollection(statement) => {
            let has_vectors = !statement.vectors.is_empty() || !statement.sparse_vectors.is_empty();
            let mode = render_collection_mode(&statement.mode);
            let mut out = format!("CREATE COLLECTION {}", render_name(&statement.collection));
            if !mode.is_empty() && (!has_vectors || statement.vectors.is_empty()) {
                let _ = write!(out, " {}", mode);
            }
            let mut defs = Vec::new();
            for vector in &statement.vectors {
                defs.push(render_vector_def(vector));
            }
            for sparse in &statement.sparse_vectors {
                defs.push(render_sparse_vector_def(sparse));
            }

            let configs = if let Some(config) = &statement.config {
                render_collection_config_clauses(config)
            } else {
                Vec::new()
            };

            let single_line_len = out.len()
                + if has_vectors {
                    defs.iter().map(|d| d.len() + 2).sum::<usize>() + 3
                } else {
                    0
                }
                + configs.iter().map(|c| c.len() + 1).sum::<usize>();

            let is_complex = defs.len() >= 2
                || (has_vectors && !configs.is_empty())
                || configs.len() >= 2
                || single_line_len > 80;

            if is_complex {
                if has_vectors {
                    out.push_str(" (\n  ");
                    out.push_str(&defs.join(",\n  "));
                    out.push_str("\n)");
                }
                for config_clause in configs {
                    out.push('\n');
                    out.push_str(&config_clause);
                }
            } else {
                if has_vectors {
                    out.push_str(" (");
                    out.push_str(&defs.join(", "));
                    out.push(')');
                }
                for config_clause in configs {
                    out.push(' ');
                    out.push_str(&config_clause);
                }
            }
            out
        }
        Stmt::CreateIndex(statement) => {
            let mut out = format!(
                "CREATE INDEX ON COLLECTION {} FOR {} TYPE {}",
                render_name(&statement.collection),
                render_name(&statement.field),
                statement.field_type
            );
            if !statement.options.is_empty() {
                let options: Vec<String> = statement
                    .options
                    .iter()
                    .map(|(key, value)| format!("{} = {}", render_name(key), render_value(value)))
                    .collect();
                let _ = write!(out, " WITH ({})", options.join(", "));
            }
            out
        }
        Stmt::DropIndex(statement) => format!(
            "DROP INDEX ON COLLECTION {} FOR {}",
            render_name(&statement.collection),
            render_name(&statement.field)
        ),
        Stmt::CreateShardKey(statement) => {
            let mut out = format!(
                "CREATE SHARD KEY '{}' ON COLLECTION {}",
                escape_string(&statement.shard_key),
                render_name(&statement.collection)
            );
            let mut options = Vec::new();
            if let Some(value) = statement.shards_number {
                options.push(format!("shards_number = {}", value));
            }
            if let Some(value) = statement.replication_factor {
                options.push(format!("replication_factor = {}", value));
            }
            if !options.is_empty() {
                let _ = write!(out, " WITH ({})", options.join(", "));
            }
            out
        }
        Stmt::DropShardKey(statement) => format!(
            "DROP SHARD KEY '{}' ON COLLECTION {}",
            escape_string(&statement.shard_key),
            render_name(&statement.collection)
        ),
        Stmt::AlterCollection(statement) => {
            let mut out = format!("ALTER COLLECTION {}", render_name(&statement.collection));
            if let Some(config) = &statement.config {
                let clauses = render_collection_config_clauses(config);
                if clauses.len() >= 2 {
                    for clause in clauses {
                        out.push('\n');
                        out.push_str(&clause);
                    }
                } else {
                    for clause in clauses {
                        out.push(' ');
                        out.push_str(&clause);
                    }
                }
            }
            out
        }
        Stmt::DropCollection(statement) => {
            format!("DROP COLLECTION {}", render_name(&statement.collection))
        }
        Stmt::ShowCollections => "SHOW COLLECTIONS".into(),
        Stmt::ShowCollection(collection) => format!("SHOW COLLECTION {}", render_name(collection)),
        Stmt::ShowShardKeys(collection) => {
            format!("SHOW SHARD KEYS ON COLLECTION {}", render_name(collection))
        }
        Stmt::ShowQuotas => "SHOW QUOTAS".into(),
        Stmt::SetQuota(stmt) => {
            let config: Vec<String> = stmt
                .config
                .iter()
                .map(|(key, value)| format!("{} = {}", render_name(key), render_value(value)))
                .collect();
            let mut out = format!("SET QUOTA ({})", config.join(", "));
            if let Some(wait) = stmt.wait {
                let _ = write!(out, " WAIT {}", wait);
            }
            out
        }
        Stmt::Delete(statement) => {
            let mut out = format!(
                "DELETE FROM {} WHERE {}",
                render_name(&statement.collection),
                render_point_selector(&statement.selector)
            );
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::ClearPayload(statement) => {
            let mut out = format!(
                "CLEAR PAYLOAD FROM {} WHERE {}",
                render_name(&statement.collection),
                render_point_selector(&statement.selector)
            );
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::DeletePayload(statement) => {
            let keys: Vec<String> = statement.keys.iter().map(|k| render_name(k)).collect();
            let mut out = format!(
                "DELETE PAYLOAD {} FROM {} WHERE {}",
                keys.join(", "),
                render_name(&statement.collection),
                render_point_selector(&statement.selector)
            );
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::DeleteVector(statement) => {
            let names: Vec<String> = statement
                .vector_names
                .iter()
                .map(|n| render_name(n))
                .collect();
            let mut out = format!(
                "DELETE VECTOR {} FROM {} WHERE {}",
                names.join(", "),
                render_name(&statement.collection),
                render_point_selector(&statement.selector)
            );
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::UpdateVector(statement) => {
            let mut out = format!("UPDATE {} SET VECTOR", render_name(&statement.collection));
            if let Some(name) = &statement.vector_name {
                let _ = write!(out, " {}", render_name(name));
            }
            let _ = write!(
                out,
                " = {} WHERE id = {}",
                render_vector_value(&statement.vector),
                render_point_id(&statement.point_id)
            );
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::UpdatePayload(statement) => {
            let payload: Vec<String> = statement
                .payload
                .iter()
                .map(|(key, value)| format!("{}: {}", render_name(key), render_value(value)))
                .collect();
            let mut out = format!(
                "UPDATE {} SET PAYLOAD = {{{}}} WHERE {}",
                render_name(&statement.collection),
                payload.join(", "),
                render_point_selector(&statement.selector)
            );
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
        Stmt::Count(statement) => {
            let mut out = match &statement.collection {
                QueryCollection::Explicit(collection) => {
                    format!("COUNT FROM {}", render_name(collection))
                }
                QueryCollection::Inherited => "COUNT FROM".into(),
            };
            if let Some(filter) = &statement.filter {
                let _ = write!(out, " WHERE {}", render_filter(filter));
            }
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            if let Some(exact) = statement.exact {
                let _ = write!(out, " WITH (exact = {})", exact);
            }
            out
        }
        Stmt::Facet(statement) => {
            let mut out = match &statement.collection {
                QueryCollection::Explicit(collection) => {
                    format!(
                        "FACET {} FROM {}",
                        render_name(&statement.key),
                        render_name(collection)
                    )
                }
                QueryCollection::Inherited => format!("FACET {}", render_name(&statement.key)),
            };
            if let Some(filter) = &statement.filter {
                let _ = write!(out, " WHERE {}", render_filter(filter));
            }
            if let Some(limit) = statement.limit {
                let _ = write!(out, " LIMIT {}", limit);
            }
            if let Some(exact) = statement.exact {
                let _ = write!(out, " EXACT {}", exact);
            }
            if let Some(key) = &statement.shard_key {
                let _ = write!(out, " SHARD '{}'", escape_string(key));
            }
            out
        }
    }
}

// ── Query rendering ──────────────────────────────────────────────

fn render_query_body(query: &QueryStmt) -> String {
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

fn query_tail_clauses(query: &QueryStmt) -> Vec<String> {
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
        parts.push(format!("SHARD '{}'", escape_string(key)));
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
        if param.starts_with('?') {
            parts.push(format!("LIMIT {}", param));
        } else {
            parts.push(format!("LIMIT :{}", param));
        }
    }
    if let Some(offset) = query.page.offset {
        parts.push(format!("OFFSET {}", offset));
    } else if let Some(param) = &query.page.offset_param {
        if param.starts_with('?') {
            parts.push(format!("OFFSET {}", param));
        } else {
            parts.push(format!("OFFSET :{}", param));
        }
    }
    parts
}

/// Render `QUERY <expr> [FROM c] <tail>` formatted with standard single-line/multiline layout.
fn render_query_body_formatted(query: &QueryStmt, has_ctes: bool) -> String {
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
fn render_query_body_inner(query: &QueryStmt) -> String {
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

fn render_query_expr(expression: &QueryExpr) -> String {
    match expression {
        QueryExpr::Points { ids } => format!(
            "POINTS ({})",
            ids.iter()
                .map(render_point_id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        QueryExpr::Nearest {
            input,
            mmr: Some(mmr),
            ..
        } => format!(
            "MMR {} DIVERSITY {} CANDIDATES {}",
            render_query_input(input, false),
            render_f64(mmr.diversity),
            mmr.candidates
        ),
        QueryExpr::Nearest { input, .. } => render_query_input(input, true),
        QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            ..
        } => {
            let mut out = format!(
                "RECOMMEND POSITIVE ({})",
                positive
                    .iter()
                    .map(render_recommend_input)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if !negative.is_empty() {
                let _ = write!(
                    out,
                    " NEGATIVE ({})",
                    negative
                        .iter()
                        .map(render_recommend_input)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if let Some(strategy) = strategy {
                let _ = write!(out, " STRATEGY {}", render_recommend_strategy(*strategy));
            }
            out
        }
        QueryExpr::Context { pairs, .. } => format!(
            "CONTEXT ({})",
            pairs
                .iter()
                .map(render_context_pair)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        QueryExpr::Discover {
            target, context, ..
        } => format!(
            "DISCOVER TARGET {} CONTEXT ({})",
            render_query_input(target, true),
            context
                .iter()
                .map(render_context_pair)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        QueryExpr::OrderBy { field, direction } => format!(
            "ORDER BY {} {}",
            render_name(field),
            match direction {
                OrderDirection::Asc => "ASC",
                OrderDirection::Desc => "DESC",
            }
        ),
        QueryExpr::SampleRandom => "SAMPLE RANDOM".into(),
        QueryExpr::Fusion { method, .. } => format!("FUSION {}", render_fusion_method(*method)),
        QueryExpr::Formula {
            expression,
            defaults,
            ..
        } => {
            let mut out = format!("FORMULA {}", render_formula(expression));
            if !defaults.is_empty() {
                let entries: Vec<String> = defaults
                    .iter()
                    .map(|(key, value)| format!("{} = {}", render_name(key), render_value(value)))
                    .collect();
                let _ = write!(out, " DEFAULTS ({})", entries.join(", "));
            }
            out
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            ..
        } => {
            let items: Vec<String> = feedback
                .iter()
                .map(|item| {
                    format!(
                        "({}, {})",
                        render_query_input(&item.example, true),
                        render_f64(item.score)
                    )
                })
                .collect();
            format!(
                "RELEVANCE FEEDBACK TARGET {} FEEDBACK ({}) STRATEGY NAIVE (a = {}, b = {}, c = {})",
                render_query_input(target, true),
                items.join(", "),
                render_f64(strategy.a),
                render_f64(strategy.b),
                render_f64(strategy.c)
            )
        }
        QueryExpr::Hybrid {
            text,
            model,
            dense_vector,
            sparse_vector,
            fusion,
        } => {
            let mut out = format!("HYBRID TEXT '{}'", escape_string(text));
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
            if let Some(vector) = dense_vector {
                let _ = write!(out, " DENSE {}", render_name(vector));
            }
            if let Some(vector) = sparse_vector {
                let _ = write!(out, " SPARSE {}", render_name(vector));
            }
            let _ = write!(out, " FUSION {}", render_fusion_method(*fusion));
            out
        }
        QueryExpr::Rerank { input, model, .. } => format!(
            "RERANK {} MODEL '{}'",
            render_query_input(input, false),
            escape_string(model)
        ),
        QueryExpr::CrossRerank {
            query,
            model,
            field,
            ..
        } => {
            let mut out = format!(
                "CROSS RERANK TEXT '{}' MODEL '{}'",
                escape_string(query),
                escape_string(model)
            );
            if let Some(field) = field {
                let _ = write!(out, " ON FIELD {}", render_name(field));
            }
            out
        }
    }
}

fn query_expr_using(expression: &QueryExpr) -> Option<&VectorTarget> {
    match expression {
        QueryExpr::Nearest { using, .. }
        | QueryExpr::Recommend { using, .. }
        | QueryExpr::Context { using, .. }
        | QueryExpr::Discover { using, .. }
        | QueryExpr::RelevanceFeedback { using, .. }
        | QueryExpr::Rerank { using, .. } => using.as_ref(),
        _ => None,
    }
}

fn query_expr_prefetch(expression: &QueryExpr) -> &[Prefetch] {
    match expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => prefetch,
        _ => &[],
    }
}

fn render_prefetch(prefetch: &Prefetch) -> String {
    let mut out = match &prefetch.source {
        PrefetchSource::Cte(name) => render_name(name),
        PrefetchSource::Query(query) => render_query_body_inner(query),
    };
    if let Some(filter) = &prefetch.filter {
        let _ = write!(out, " WHERE {}", render_filter(filter));
    }
    if let Some(score) = prefetch.score_threshold {
        let _ = write!(out, " SCORE THRESHOLD {}", render_f64(score));
    }
    if let Some(lookup) = &prefetch.lookup {
        let _ = write!(out, " LOOKUP FROM {}", render_name(&lookup.collection));
        if let Some(vector) = &lookup.vector {
            let _ = write!(out, " VECTOR {}", render_name(vector));
        }
    }
    out
}

fn render_query_input(input: &QueryInput, allow_bare: bool) -> String {
    match input {
        QueryInput::Text { text, model: None } if allow_bare => {
            if text.starts_with(':') || text.starts_with('?') {
                text.clone()
            } else {
                format!("'{}'", escape_string(text))
            }
        }
        QueryInput::Text { text, model } => {
            let rendered_text = if text.starts_with(':') || text.starts_with('?') {
                text.clone()
            } else {
                format!("'{}'", escape_string(text))
            };
            let mut out = format!("TEXT {}", rendered_text);
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
            out
        }
        QueryInput::Image { source, model } => {
            let mut out = format!("IMAGE '{}'", escape_string(source));
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
            out
        }
        QueryInput::Vector(value) => format!("VECTOR {}", render_vector_value(value)),
        QueryInput::Point(point) => format!("POINT {}", render_point_id(point)),
        QueryInput::Param(name) => format!(":{}", name),
        QueryInput::PositionalParam(_) => "?".to_string(),
    }
}

fn render_recommend_input(input: &QueryInput) -> String {
    match input {
        // `RECOMMEND POSITIVE (...)` is a point-id list — no `POINT` keyword.
        QueryInput::Point(point) => render_point_id(point),
        other => render_query_input(other, true),
    }
}

fn render_context_pair(pair: &ContextPair) -> String {
    format!(
        "POSITIVE {} NEGATIVE {}",
        render_query_input(&pair.positive, true),
        render_query_input(&pair.negative, true)
    )
}

fn render_vector_target(target: &VectorTarget) -> String {
    let mut out = render_name(&target.name);
    if target.multi {
        out.push_str(" AS MULTI");
    } else if let Some(kind) = target.kind {
        out.push_str(match kind {
            VectorKind::Dense => " AS DENSE",
            VectorKind::Sparse => " AS SPARSE",
        });
    }
    out
}

fn render_recommend_strategy(strategy: RecommendStrategy) -> &'static str {
    match strategy {
        RecommendStrategy::AverageVector => "average_vector",
        RecommendStrategy::BestScore => "best_score",
        RecommendStrategy::SumScores => "sum_scores",
    }
}

fn render_fusion_method(method: FusionMethod) -> &'static str {
    match method {
        FusionMethod::Rrf => "RRF",
        FusionMethod::Dbsf => "DBSF",
    }
}

// ── Search params / selectors ────────────────────────────────────

pub(crate) fn render_search_params(params: &SearchParams) -> String {
    let mut parts = Vec::new();
    if let Some(value) = params.hnsw_ef {
        parts.push(format!("hnsw_ef = {}", value));
    }
    if let Some(value) = params.exact {
        parts.push(format!("exact = {}", value));
    }
    if let Some(value) = params.acorn {
        parts.push(format!("acorn = {}", value));
    }
    if let Some(value) = params.max_selectivity {
        parts.push(format!("max_selectivity = {}", render_f64(value)));
    }
    if let Some(value) = params.indexed_only {
        parts.push(format!("indexed_only = {}", value));
    }
    if let Some(value) = params.rrf_k {
        parts.push(format!("rrf_k = {}", value));
    }
    if let Some(values) = &params.rrf_weights {
        parts.push(format!(
            "rrf_weights = [{}]",
            values
                .iter()
                .map(|v| render_f64(*v))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(quantization) = &params.quantization {
        let mut entries = Vec::new();
        if let Some(value) = quantization.ignore {
            entries.push(format!("ignore: {}", value));
        }
        if let Some(value) = quantization.rescore {
            entries.push(format!("rescore: {}", value));
        }
        if let Some(value) = quantization.oversampling {
            entries.push(format!("oversampling: {}", render_f64(value)));
        }
        parts.push(format!("quantization = {{{}}}", entries.join(", ")));
    }
    if let Some(idf) = &params.idf {
        match &idf.corpus {
            None => parts.push("idf = 'global'".into()),
            Some(filter) => parts.push(format!("idf = WHERE {}", render_filter(filter))),
        }
    }
    if let Some(value) = params.timeout {
        parts.push(format!("timeout = {}", value));
    }
    if let Some(consistency) = &params.consistency {
        parts.push(format!(
            "consistency = {}",
            render_read_consistency(consistency)
        ));
    }
    parts.join(", ")
}

fn render_read_consistency(consistency: &ReadConsistency) -> String {
    match consistency {
        ReadConsistency::Factor(value) => value.to_string(),
        ReadConsistency::Majority => "majority".into(),
        ReadConsistency::Quorum => "quorum".into(),
        ReadConsistency::All => "all".into(),
    }
}

fn render_payload_selector(selector: &PayloadSelector) -> String {
    match selector {
        PayloadSelector::All => "true".into(),
        PayloadSelector::None => "false".into(),
        PayloadSelector::Include(names) => format!(
            "INCLUDE ({})",
            names
                .iter()
                .map(|n| render_name(n))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        PayloadSelector::Exclude(names) => format!(
            "EXCLUDE ({})",
            names
                .iter()
                .map(|n| render_name(n))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_vector_selector(selector: &VectorSelector) -> String {
    match selector {
        VectorSelector::All => "true".into(),
        VectorSelector::None => "false".into(),
        VectorSelector::Names(names) => format!(
            "({})",
            names
                .iter()
                .map(|n| render_name(n))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

// ── Filters ──────────────────────────────────────────────────────

pub(crate) fn render_filter(filter: &FilterExpr) -> String {
    match filter {
        FilterExpr::And { operands } => operands
            .iter()
            .map(|operand| {
                if matches!(operand, FilterExpr::And { .. } | FilterExpr::Or { .. }) {
                    format!("({})", render_filter(operand))
                } else {
                    render_filter(operand)
                }
            })
            .collect::<Vec<_>>()
            .join(" AND "),
        FilterExpr::Or { operands } => operands
            .iter()
            .map(|operand| {
                if matches!(operand, FilterExpr::Or { .. }) {
                    format!("({})", render_filter(operand))
                } else {
                    render_filter(operand)
                }
            })
            .collect::<Vec<_>>()
            .join(" OR "),
        FilterExpr::Not { operand } => {
            if matches!(
                operand.as_ref(),
                FilterExpr::And { .. } | FilterExpr::Or { .. }
            ) {
                format!("NOT ({})", render_filter(operand))
            } else {
                format!("NOT {}", render_filter(operand))
            }
        }
        predicate => render_filter_predicate(predicate),
    }
}

fn render_filter_predicate(filter: &FilterExpr) -> String {
    match filter {
        FilterExpr::PointId(PointIdPredicate::Eq(point)) => {
            format!("id = {}", render_point_id(point))
        }
        FilterExpr::PointId(PointIdPredicate::In(points)) => format!(
            "id IN ({})",
            points
                .iter()
                .map(render_point_id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FilterExpr::Compare { field, op, value } => format!(
            "{} {} {}",
            render_name(field),
            render_comparison_op(*op),
            render_value(value)
        ),
        FilterExpr::Between { field, low, high } => format!(
            "{} BETWEEN {} AND {}",
            render_name(field),
            render_value(low),
            render_value(high)
        ),
        FilterExpr::In { field, values } => format!(
            "{} IN ({})",
            render_name(field),
            values
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FilterExpr::IsNull { field } => format!("{} IS NULL", render_name(field)),
        FilterExpr::IsEmpty { field } => format!("{} IS EMPTY", render_name(field)),
        FilterExpr::MatchText { field, text } => {
            format!("{} MATCH '{}'", render_name(field), escape_string(text))
        }
        FilterExpr::MatchAny { field, values } => format!(
            "{} MATCH ANY ({})",
            render_name(field),
            values
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FilterExpr::MatchPhrase { field, text } => format!(
            "{} MATCH PHRASE '{}'",
            render_name(field),
            escape_string(text)
        ),
        FilterExpr::MatchPrefix { field, prefix } => format!(
            "{} MATCH PREFIX '{}'",
            render_name(field),
            escape_string(prefix)
        ),
        FilterExpr::Nested { path, filter } => format!(
            "NESTED('{}', {})",
            escape_string(path),
            render_filter(filter)
        ),
        FilterExpr::HasVector { name } => format!("HAS_VECTOR {}", render_name(name)),
        FilterExpr::Slice { total, index } => format!("SLICE ({}, {})", total, index),
        FilterExpr::ValuesCount { field, op, count } => format!(
            "{} VALUES_COUNT {} {}",
            render_name(field),
            render_comparison_op(*op),
            count
        ),
        FilterExpr::GeoBoundingBox {
            field,
            top_left,
            bottom_right,
        } => format!(
            "{} GEO_BBOX {{top_left: {{lat: {}, lon: {}}}, bottom_right: {{lat: {}, lon: {}}}}}",
            render_name(field),
            render_f64(top_left.lat),
            render_f64(top_left.lon),
            render_f64(bottom_right.lat),
            render_f64(bottom_right.lon)
        ),
        FilterExpr::GeoRadius {
            field,
            center,
            radius,
        } => format!(
            "{} GEO_RADIUS {{center: {{lat: {}, lon: {}}}, radius: {}}}",
            render_name(field),
            render_f64(center.lat),
            render_f64(center.lon),
            render_f64(*radius)
        ),
        FilterExpr::GeoPolygon {
            field,
            exterior,
            interiors,
        } => {
            let mut out = format!(
                "{} GEO_POLYGON {{exterior: [{}]",
                render_name(field),
                render_geo_ring(exterior)
            );
            if !interiors.is_empty() {
                let _ = write!(
                    out,
                    ", interiors: [{}]",
                    interiors
                        .iter()
                        .map(|ring| format!("[{}]", render_geo_ring(ring)))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            out.push('}');
            out
        }
        _ => render_filter(filter),
    }
}

fn render_geo_ring(points: &[GeoPoint]) -> String {
    points
        .iter()
        .map(|point| {
            format!(
                "{{lat: {}, lon: {}}}",
                render_f64(point.lat),
                render_f64(point.lon)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_comparison_op(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::Eq => "=",
        ComparisonOp::Gt => ">",
        ComparisonOp::Gte => ">=",
        ComparisonOp::Lt => "<",
        ComparisonOp::Lte => "<=",
    }
}

// ── Formulas ─────────────────────────────────────────────────────

fn render_formula(formula: &FormulaExpr) -> String {
    render_formula_min(formula, 0)
}

fn formula_precedence(formula: &FormulaExpr) -> u8 {
    match formula {
        FormulaExpr::Sum { .. } | FormulaExpr::Sub { .. } => 1,
        FormulaExpr::Mul { .. } | FormulaExpr::Div { .. } => 2,
        FormulaExpr::Neg { .. } => 3,
        _ => 4,
    }
}

fn render_formula_min(formula: &FormulaExpr, min_precedence: u8) -> String {
    let precedence = formula_precedence(formula);
    let rendered = match formula {
        FormulaExpr::Constant { value } => render_f64(*value),
        FormulaExpr::Variable { name } => name.clone(),
        FormulaExpr::Sum { left, right } => format!(
            "{} + {}",
            render_formula_min(left, 1),
            render_formula_min(right, 2)
        ),
        FormulaExpr::Sub { left, right } => format!(
            "{} - {}",
            render_formula_min(left, 1),
            render_formula_min(right, 2)
        ),
        FormulaExpr::Mul { left, right } => format!(
            "{} * {}",
            render_formula_min(left, 2),
            render_formula_min(right, 3)
        ),
        FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let mut out = format!(
                "{} / {}",
                render_formula_min(left, 2),
                render_formula_min(right, 3)
            );
            if let Some(default) = by_zero_default {
                let _ = write!(out, " [DEFAULT = {}]", render_f64(*default));
            }
            out
        }
        FormulaExpr::Neg { operand } => format!("-{}", render_formula_min(operand, 3)),
        FormulaExpr::Abs { x } => format!("ABS({})", render_formula_min(x, 0)),
        FormulaExpr::Sqrt { x } => format!("SQRT({})", render_formula_min(x, 0)),
        FormulaExpr::Log { x } => format!("LOG({})", render_formula_min(x, 0)),
        FormulaExpr::Ln { x } => format!("LN({})", render_formula_min(x, 0)),
        FormulaExpr::Exp { x } => format!("EXP({})", render_formula_min(x, 0)),
        FormulaExpr::Acosh { x } => format!("ACOSH({})", render_formula_min(x, 0)),
        FormulaExpr::Max { args } => render_formula_call("MAX", args),
        FormulaExpr::Min { args } => render_formula_call("MIN", args),
        FormulaExpr::Pow { base, exponent } => format!(
            "POW({}, {})",
            render_formula_min(base, 0),
            render_formula_min(exponent, 0)
        ),
        FormulaExpr::GeoDistance { lat, lon, field } => format!(
            "GEO_DISTANCE({}, {}, {})",
            render_f64(*lat),
            render_f64(*lon),
            render_name(field)
        ),
        FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let mut out = format!("{}(", kind.to_ascii_uppercase());
            out.push_str(&render_formula_min(x, 0));
            if let Some(target) = target {
                let _ = write!(out, ", TARGET = {}", render_formula_min(target, 0));
            }
            if let Some(scale) = scale {
                let _ = write!(out, ", SCALE = {}", render_f64(*scale));
            }
            if let Some(midpoint) = midpoint {
                let _ = write!(out, ", MIDPOINT = {}", render_f64(*midpoint));
            }
            out.push(')');
            out
        }
        FormulaExpr::Case { cond, then_, else_ } => format!(
            "CASE WHEN {} THEN {} ELSE {} END",
            render_filter(cond),
            render_formula_min(then_, 0),
            render_formula_min(else_, 0)
        ),
        FormulaExpr::MatchCondition { field, values } => {
            if values.len() == 1 {
                format!(
                    "MATCH({}, {})",
                    render_name(field),
                    render_value(&values[0])
                )
            } else {
                format!(
                    "MATCH({}, [{}])",
                    render_name(field),
                    values
                        .iter()
                        .map(render_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        FormulaExpr::Datetime { value } => format!("datetime('{}')", escape_string(value)),
        FormulaExpr::DatetimeKey { key } => format!("datetime_key('{}')", escape_string(key)),
    };
    if precedence < min_precedence {
        format!("({})", rendered)
    } else {
        rendered
    }
}

/// Render an n-ary formula call (`MAX(a, b)`, `MIN(a, b, c)`).
fn render_formula_call(name: &str, args: &[FormulaExpr]) -> String {
    let rendered: Vec<String> = args.iter().map(|a| render_formula_min(a, 0)).collect();
    format!("{}({})", name, rendered.join(", "))
}

// ── DDL config blocks ────────────────────────────────────────────

fn render_collection_config_clauses(config: &CollectionConfig) -> Vec<String> {
    let mut clauses = Vec::new();
    if let Some(hnsw) = &config.hnsw
        && let Some(body) = render_hnsw_block(hnsw)
    {
        clauses.push(format!("WITH HNSW ({})", body));
    }
    if let Some(vectors) = &config.vectors
        && let Some(body) = render_vectors_options(vectors)
    {
        clauses.push(format!("WITH VECTOR ({})", body));
    }
    if let Some(optimizers) = &config.optimizers
        && let Some(body) = render_optimizers_block(optimizers)
    {
        clauses.push(format!("WITH OPTIMIZERS ({})", body));
    }
    if let Some(params) = &config.params
        && let Some(body) = render_params_block(params)
    {
        clauses.push(format!("WITH PARAMS ({})", body));
    }
    if let Some(quantization) = &config.quantization
        && let Some(body) = render_quantization_block(quantization)
    {
        clauses.push(format!("WITH QUANTIZATION ({})", body));
    }
    if let Some(update) = &config.quantization_update {
        if update.disabled {
            clauses.push("WITH QUANTIZATION (disabled = true)".into());
        } else if config.quantization.is_none()
            && let Some(config) = &update.config
            && let Some(body) = render_quantization_block(config)
        {
            clauses.push(format!("WITH QUANTIZATION ({})", body));
        }
    }
    clauses
}

fn render_collection_mode(mode: &CollectionMode) -> String {
    match mode {
        CollectionMode::Dense { model: Some(model) } => {
            format!("USING DENSE MODEL '{}'", escape_string(model))
        }
        CollectionMode::Dense { model: None } => String::new(),
        CollectionMode::Hybrid {
            dense_vector,
            sparse_vector,
        } => {
            let mut out = String::from("HYBRID");
            if let Some(vector) = dense_vector {
                let _ = write!(out, " DENSE VECTOR {}", render_name(vector));
            }
            if let Some(vector) = sparse_vector {
                let _ = write!(out, " SPARSE VECTOR {}", render_name(vector));
            }
            out
        }
        CollectionMode::Rerank => "HYBRID RERANK".into(),
    }
}

fn render_vector_def(vector: &VectorDef) -> String {
    let mut out = format!(
        "{} VECTOR({}, {})",
        render_name(&vector.name),
        vector.size,
        render_distance(vector.distance)
    );
    if let Some(hnsw) = &vector.hnsw
        && let Some(body) = render_hnsw_block(hnsw)
    {
        let _ = write!(out, " WITH HNSW ({})", body);
    }
    if let Some(quantization) = &vector.quantization
        && let Some(body) = render_quantization_block(quantization)
    {
        let _ = write!(out, " WITH QUANTIZATION ({})", body);
    }
    if let Some(multivector) = &vector.multivector {
        let _ = write!(
            out,
            " WITH MULTIVECTOR (comparator = '{}')",
            match multivector.comparator {
                MultivectorComparator::MaxSim => "max_sim",
            }
        );
    }
    if let Some(vectors) = &vector.vectors
        && let Some(body) = render_vectors_options(vectors)
    {
        let _ = write!(out, " WITH VECTOR ({})", body);
    }
    out
}

fn render_vectors_options(vectors: &VectorsConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = vectors.on_disk {
        options.push(format!("on_disk = {}", value));
    }
    if let Some(value) = vectors.memory {
        options.push(format!("memory = '{}'", value.as_str()));
    }
    if let Some(value) = vectors.datatype {
        options.push(format!("datatype = '{}'", value.as_str()));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

fn render_sparse_vector_def(vector: &SparseVectorDef) -> String {
    let mut out = format!("{} SPARSE", render_name(&vector.name));
    let mut options = Vec::new();
    if let Some(modifier) = &vector.modifier {
        options.push(format!(
            "modifier = '{}'",
            escape_string(&modifier.to_ascii_lowercase())
        ));
    }
    if let Some(index) = &vector.index {
        if let Some(value) = index.full_scan_threshold {
            options.push(format!("full_scan_threshold = {}", value));
        }
        if let Some(value) = index.on_disk {
            options.push(format!("on_disk = {}", value));
        }
        if let Some(value) = index.datatype {
            options.push(format!("datatype = '{}'", value.as_str()));
        }
        if let Some(value) = index.memory {
            options.push(format!("memory = '{}'", value.as_str()));
        }
    }
    if !options.is_empty() {
        let _ = write!(out, " WITH SPARSE ({})", options.join(", "));
    }
    out
}

fn render_hnsw_block(hnsw: &HnswRuntimeConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = hnsw.m {
        options.push(format!("m = {}", value));
    }
    if let Some(value) = hnsw.ef_construct {
        options.push(format!("ef_construct = {}", value));
    }
    if let Some(value) = hnsw.full_scan_threshold {
        options.push(format!("full_scan_threshold = {}", value));
    }
    if let Some(value) = hnsw.max_indexing_threads {
        options.push(format!("max_indexing_threads = {}", value));
    }
    if let Some(value) = hnsw.on_disk {
        options.push(format!("on_disk = {}", value));
    }
    if let Some(value) = hnsw.payload_m {
        options.push(format!("payload_m = {}", value));
    }
    if let Some(value) = hnsw.inline_storage {
        options.push(format!("inline_storage = {}", value));
    }
    if let Some(value) = hnsw.memory {
        options.push(format!("memory = '{}'", value.as_str()));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

fn render_optimizers_block(optimizers: &OptimizersRuntimeConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = optimizers.deleted_threshold {
        options.push(format!("deleted_threshold = {}", render_f64(value)));
    }
    if let Some(value) = optimizers.vacuum_min_vector_number {
        options.push(format!("vacuum_min_vector_number = {}", value));
    }
    if let Some(value) = optimizers.default_segment_number {
        options.push(format!("default_segment_number = {}", value));
    }
    if let Some(value) = optimizers.max_segment_size {
        options.push(format!("max_segment_size = {}", value));
    }
    if let Some(value) = optimizers.memmap_threshold {
        options.push(format!("memmap_threshold = {}", value));
    }
    if let Some(value) = optimizers.indexing_threshold {
        options.push(format!("indexing_threshold = {}", value));
    }
    if let Some(value) = optimizers.flush_interval_sec {
        options.push(format!("flush_interval_sec = {}", value));
    }
    if let Some(threads) = &optimizers.max_optimization_threads {
        if threads.auto_ {
            options.push("max_optimization_threads = 'auto'".into());
        } else {
            options.push(format!("max_optimization_threads = {}", threads.value));
        }
    }
    if let Some(value) = optimizers.prevent_unoptimized {
        options.push(format!("prevent_unoptimized = {}", value));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

fn render_params_block(params: &CollectionParamsConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = params.replication_factor {
        options.push(format!("replication_factor = {}", value));
    }
    if let Some(value) = params.write_consistency_factor {
        options.push(format!("write_consistency_factor = {}", value));
    }
    if let Some(value) = params.read_fan_out_factor {
        options.push(format!("read_fan_out_factor = {}", value));
    }
    if let Some(value) = params.read_fan_out_delay_ms {
        options.push(format!("read_fan_out_delay_ms = {}", value));
    }
    if let Some(value) = params.on_disk_payload {
        options.push(format!("on_disk_payload = {}", value));
    }
    if let Some(value) = params.payload_memory {
        options.push(format!("payload_memory = '{}'", value.as_str()));
    }
    if let Some(value) = params.shard_number {
        options.push(format!("shard_number = {}", value));
    }
    if let Some(value) = &params.sharding_method {
        options.push(format!(
            "sharding_method = '{}'",
            escape_string(&value.to_ascii_lowercase())
        ));
    }
    if let Some(values) = &params.shard_keys {
        options.push(format!(
            "shard_keys = [{}]",
            values
                .iter()
                .map(|s| format!("'{}'", escape_string(s)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

fn render_quantization_block(quantization: &QuantizationConfig) -> Option<String> {
    let mut options = vec![format!(
        "type = '{}'",
        render_quantization_type(quantization.qtype)
    )];
    if quantization.always_ram {
        options.push("always_ram = true".into());
    }
    if let Some(value) = quantization.quantile {
        options.push(format!("quantile = {}", render_f64(value)));
    }
    if let Some(value) = quantization.bits {
        options.push(format!("bits = {}", render_f64(value)));
    }
    if let Some(value) = &quantization.compression {
        options.push(format!("compression = '{}'", escape_string(value)));
    }
    if let Some(value) = &quantization.encoding {
        options.push(format!("encoding = '{}'", escape_string(value)));
    }
    if let Some(value) = &quantization.query_encoding {
        options.push(format!("query_encoding = '{}'", escape_string(value)));
    }
    if let Some(value) = quantization.memory {
        options.push(format!("memory = '{}'", value.as_str()));
    }
    Some(options.join(", "))
}

fn render_quantization_type(kind: QuantizationType) -> &'static str {
    match kind {
        QuantizationType::Scalar => "scalar",
        QuantizationType::Binary => "binary",
        QuantizationType::Product => "product",
        QuantizationType::Turbo => "turbo",
    }
}

fn render_distance(distance: VectorDistance) -> &'static str {
    match distance {
        VectorDistance::Cosine => "COSINE",
        VectorDistance::Dot => "DOT",
        VectorDistance::Euclid => "EUCLID",
        VectorDistance::Manhattan => "MANHATTAN",
    }
}

// ── Points, values, vectors ──────────────────────────────────────

fn render_point_selector(selector: &PointSelector) -> String {
    match selector {
        PointSelector::Id(point) => format!("id = {}", render_point_id(point)),
        PointSelector::Ids(points) => format!(
            "id IN ({})",
            points
                .iter()
                .map(render_point_id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        PointSelector::Filter(filter) => render_filter(filter),
    }
}

fn render_point(point: &UpsertPoint) -> String {
    let mut parts = vec![format!("id: {}", render_point_id(&point.id))];
    if let Some(vectors) = &point.vectors {
        parts.push(format!("vector: {}", render_point_vectors(vectors)));
    }
    for (key, value) in &point.payload {
        parts.push(format!("{}: {}", render_name(key), render_value(value)));
    }
    format!("{{{}}}", parts.join(", "))
}

fn render_point_vectors(vectors: &PointVectors) -> String {
    match vectors {
        PointVectors::Unnamed(value) => render_vector_value(value),
        PointVectors::Named(pairs) => {
            let entries: Vec<String> = pairs
                .iter()
                .map(|(name, value)| {
                    format!("{}: {}", render_name(name), render_vector_value(value))
                })
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
    }
}

fn render_point_id(point: &PointId) -> String {
    match point {
        PointId::Number(value) => value.to_string(),
        PointId::String(value) => format!("'{}'", escape_string(value)),
        PointId::Param(name) => format!(":{}", name),
        PointId::PositionalParam(_) => "?".to_string(),
    }
}

fn render_vector_value(value: &VectorValue) -> String {
    match value {
        VectorValue::Dense(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|v| render_f32(*v))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        VectorValue::Sparse { indices, values } => format!(
            "{{indices: [{}], values: [{}]}}",
            indices
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            values
                .iter()
                .map(|v| render_f32(*v))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        VectorValue::MultiDense(rows) => format!(
            "[{}]",
            rows.iter()
                .map(|row| format!(
                    "[{}]",
                    row.iter()
                        .map(|v| render_f32(*v))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Str(value) => format!("'{}'", escape_string(value)),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => render_f64(*value),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".into(),
        Value::Dict(entries) => {
            let items: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{}: {}", render_name(key), render_value(value)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        Value::List(items) => {
            let values: Vec<String> = items.iter().map(render_value).collect();
            format!("[{}]", values.join(", "))
        }
        Value::Param(name) => format!(":{}", name),
        Value::PositionalParam(_) => "?".to_string(),
    }
}

/// Render a single statement in human-readable QQL form with compact vector literals.
pub fn format_stmt_readable(statement: &Stmt) -> String {
    crate::params::truncate_vector_literals(&format_stmt(statement), 5)
}

fn render_f32(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e7 {
        format!("{:.1}", value)
    } else {
        let rendered = value.to_string();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{}.0", rendered)
        }
    }
}

fn render_f64(value: f64) -> String {
    // Keep integral floats as floats (`1.0`), not integers, so the literal
    // re-parses to the same `Value::Float` / `FormulaExpr::Constant`.
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{:.1}", value)
    } else {
        let rendered = value.to_string();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{}.0", rendered)
        }
    }
}

// ── Upsert embedding specs ───────────────────────────────────────

fn render_embedding_spec(spec: &EmbeddingSpec) -> String {
    match spec {
        EmbeddingSpec::Dense {
            model,
            vector,
            field,
        } => render_embedding_spec_part("DENSE", model, vector, field, false),
        EmbeddingSpec::Sparse {
            model,
            vector,
            field,
        } => render_embedding_spec_part("SPARSE", model, vector, field, false),
        EmbeddingSpec::MultiVector {
            model,
            vector,
            field,
        } => render_embedding_spec_part("MULTIVECTOR", model, vector, field, false),
        EmbeddingSpec::Image {
            model,
            vector,
            field,
        } => render_embedding_spec_part("IMAGE", model, vector, field, false),
        EmbeddingSpec::Hybrid {
            dense_model,
            dense_vector,
            dense_field,
            sparse_model,
            sparse_vector,
            sparse_field,
        } => {
            let mut parts = vec!["HYBRID".to_string()];
            if dense_model.is_some() || dense_vector.is_some() || dense_field.is_some() {
                parts.push(render_embedding_spec_part(
                    "DENSE",
                    dense_model,
                    dense_vector,
                    dense_field,
                    true,
                ));
            }
            if sparse_model.is_some() || sparse_vector.is_some() || sparse_field.is_some() {
                parts.push(render_embedding_spec_part(
                    "SPARSE",
                    sparse_model,
                    sparse_vector,
                    sparse_field,
                    true,
                ));
            }
            parts.join(" ")
        }
        EmbeddingSpec::Multi(specs) => specs
            .iter()
            .map(render_embedding_spec)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn render_embedding_spec_part(
    kind: &str,
    model: &Option<String>,
    vector: &Option<String>,
    field: &Option<String>,
    hybrid: bool,
) -> String {
    let mut parts = vec![kind.to_string()];
    if let Some(model) = model {
        parts.push(format!("MODEL '{}'", escape_string(model)));
    }
    if let Some(field) = field {
        parts.push(format!("ON FIELD {}", render_name(field)));
    }
    if let Some(vector) = vector {
        parts.push(format!(
            "{} {}",
            if hybrid { "VECTOR" } else { "INTO" },
            render_name(vector)
        ));
    }
    parts.join(" ")
}

fn render_embed_directive(directive: &EmbedDirective) -> String {
    let mut out = format!(
        "{} INTO {} USING ",
        render_name(&directive.source_field),
        render_name(&directive.target_vector)
    );
    match &directive.kind {
        EmbedKind::Dense { model } => {
            out.push_str("DENSE");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
        EmbedKind::Sparse { model } => {
            out.push_str("SPARSE");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
        EmbedKind::Multi { model } => {
            out.push_str("MULTI");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
        EmbedKind::Image { model } => {
            out.push_str("IMAGE");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
    }
    out
}

// ── Identifiers / escaping ───────────────────────────────────────

/// Format a collection/field/vector name. Simple idents stay bare; anything
/// else (dotted paths, `$`-prefixed variables, hyphens, keywords) is quoted.
fn render_name(name: &str) -> String {
    if is_simple_ident(name) {
        name.to_string()
    } else {
        format!("'{}'", escape_string(name))
    }
}

fn is_simple_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Escape a string for a QQL single-quoted literal.
///
/// Only escape sequences the parser decodes are emitted (`\\`, `\'`, `\n`,
/// `\r`, `\t`), so the rendered literal always re-parses to the same content.
/// Null bytes are dropped (the parser has no escape for them).
fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_f32_precision() {
        assert_eq!(render_f32(0.1), "0.1");
        assert_eq!(render_f32(0.2), "0.2");
        assert_eq!(render_f32(0.3), "0.3");
        assert_eq!(render_f32(1.0), "1.0");
        assert_eq!(render_f32(0.0), "0.0");
        assert_eq!(render_f32(-0.5), "-0.5");
    }

    #[test]
    fn test_vector_formatting_precision() {
        let input = "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs LIMIT 10;";
        let formatted = format(input).unwrap();
        assert_eq!(
            formatted,
            "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs LIMIT 10;\n"
        );
        let twice = format(&formatted).unwrap();
        assert_eq!(formatted, twice);
    }

    #[test]
    fn test_format_preserves_comments_and_blank_lines() {
        let input = r#"-- Header comment line 1
-- Header comment line 2

-- Section 1
COUNT FROM docs;

-- Section 2
COUNT FROM docs WHERE status = 'active'; -- trailing comment
"#;
        let formatted = format(input).unwrap();
        assert_eq!(formatted, input);
        let twice = format(&formatted).unwrap();
        assert_eq!(formatted, twice);
    }

    #[test]
    fn test_format_collapses_multiple_blank_lines() {
        let input = r#"COUNT FROM docs;




COUNT FROM other;
"#;
        let formatted = format(input).unwrap();
        let expected = "COUNT FROM docs;\n\nCOUNT FROM other;\n";
        assert_eq!(formatted, expected);
        let twice = format(&formatted).unwrap();
        assert_eq!(formatted, twice);
    }

    #[test]
    fn test_format_inline_comment_inside_statement() {
        let input =
            "QUERY 'search' FROM docs\n-- filter status\nWHERE status = 'published' LIMIT 10;\n";
        let formatted = format(input).unwrap();
        let expected =
            "-- filter status\nQUERY 'search' FROM docs WHERE status = 'published' LIMIT 10;\n";
        assert_eq!(formatted, expected);
        let twice = format(&formatted).unwrap();
        assert_eq!(formatted, twice);
    }
}
