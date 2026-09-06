//! Human-readable query plan inspection.
//!
//! Provides structured tree-formatted explanations of parsed QQL statements,
//! detailing query execution intent, CTE subqueries, target vector spaces,
//! routing keys, filter trees, and parameter configurations.

use crate::ast::*;
use crate::error::QqlError;
use crate::fmt::{render_filter, render_search_params};
use crate::parser::Parser;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Parse `source` and return a structured execution plan.
pub fn explain(source: &str) -> Result<String, QqlError> {
    let statement = Parser::parse(source)?;
    Ok(explain_node(&statement))
}

/// Explain every statement in a semicolon-delimited script.
/// Returns a concatenated plan, one section per statement.
pub fn explain_all(source: &str) -> Result<String, QqlError> {
    let statements = Parser::parse_all(source)?;
    Ok(explain_nodes(&statements))
}

/// Explain an already parsed sequence without parsing the source again.
pub fn explain_nodes(statements: &[Stmt]) -> String {
    if statements.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for (i, stmt) in statements.iter().enumerate() {
        if i > 0 {
            output.push('\n');
        }
        let _ = writeln!(output, "── Statement {} ──", i + 1);
        output.push_str(&explain_node(stmt));
    }
    output
}

/// Render a single AST node as a tree-structured plan.
pub fn explain_node(statement: &Stmt) -> String {
    let mut output = String::new();
    match statement {
        Stmt::Query(query) => {
            let intent = query_intent(&query.expression);
            let _ = writeln!(output, "Statement: QUERY [{}]", intent);

            let col = match &query.collection {
                QueryCollection::Explicit(collection) => collection.as_str(),
                QueryCollection::Inherited => "inherited",
            };
            let _ = writeln!(output, "├── Collection: {}", col);

            if let Some(target) = query_target_vector(&query.expression) {
                let _ = writeln!(output, "├── Target Vector: {}", target);
            }

            if let Some(shard) = &query.shard_key {
                let _ = writeln!(output, "├── Shard Key: '{}'", shard);
            }

            if !query.ctes.is_empty() {
                let _ = writeln!(output, "├── CTEs ({}):", query.ctes.len());
                for (i, cte) in query.ctes.iter().enumerate() {
                    let is_last = i + 1 == query.ctes.len();
                    let prefix = if is_last {
                        "│   └──"
                    } else {
                        "│   ├──"
                    };
                    let _ = writeln!(
                        output,
                        "{} '{}': {}",
                        prefix,
                        cte.name,
                        query_intent(&cte.query.expression)
                    );
                }
            }

            if let Some(prefetches) = query_prefetches(&query.expression)
                && !prefetches.is_empty()
            {
                let _ = writeln!(output, "├── Prefetches ({}):", prefetches.len());
                for (i, pf) in prefetches.iter().enumerate() {
                    let is_last = i + 1 == prefetches.len();
                    let prefix = if is_last {
                        "│   └──"
                    } else {
                        "│   ├──"
                    };
                    let _ = writeln!(output, "{} [{}] {}", prefix, i + 1, pf);
                }
            }

            if let Some(filter) = &query.filter {
                let _ = writeln!(output, "├── Filter: {}", render_filter(filter));
            }

            if let Some(params) = &query.params {
                let _ = writeln!(
                    output,
                    "├── Search Params: {}",
                    render_search_params(params)
                );
            }

            if let Some(score) = query.score_threshold {
                let _ = writeln!(output, "├── Score Threshold: {}", score);
            }

            if let Some(group) = &query.group {
                let size_str = group
                    .size
                    .map(|s| format!(" (size: {})", s))
                    .unwrap_or_default();
                let _ = writeln!(output, "├── Group By: {}{}", group.field, size_str);
            }

            if let Some(payload) = &query.output.payload {
                match payload {
                    PayloadSelector::Include(keys) => {
                        let _ = writeln!(output, "├── Payload: INCLUDE ({})", keys.join(", "));
                    }
                    PayloadSelector::Exclude(keys) => {
                        let _ = writeln!(output, "├── Payload: EXCLUDE ({})", keys.join(", "));
                    }
                    PayloadSelector::All => {
                        output.push_str("├── Payload: ALL\n");
                    }
                    PayloadSelector::None => {
                        output.push_str("├── Payload: NONE\n");
                    }
                }
            }

            if let Some(vectors) = &query.output.vectors {
                match vectors {
                    VectorSelector::Names(names) => {
                        let _ = writeln!(output, "├── Vectors: ({})", names.join(", "));
                    }
                    VectorSelector::All => {
                        output.push_str("├── Vectors: ALL\n");
                    }
                    VectorSelector::None => {
                        output.push_str("├── Vectors: NONE\n");
                    }
                }
            }

            let limit = query
                .page
                .limit
                .map(|l| l.to_string())
                .unwrap_or_else(|| "default".into());
            let offset = query.page.offset.unwrap_or(0);
            let _ = writeln!(output, "└── Pagination: limit={}, offset={}", limit, offset);
        }
        Stmt::Scroll(statement) => {
            output.push_str("Statement: SCROLL\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            if let Some(f) = &statement.filter {
                let _ = writeln!(output, "├── Filter: {}", render_filter(f));
            }
            if let Some(shard) = &statement.shard_key {
                let _ = writeln!(output, "├── Shard Key: '{}'", shard);
            }
            let _ = writeln!(output, "└── Limit: {}", statement.limit);
        }
        Stmt::Upsert(statement) => {
            output.push_str("Statement: UPSERT\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            let _ = writeln!(output, "├── Points: {}", statement.points.len());
            if let Some(shard) = &statement.shard_key {
                let _ = writeln!(output, "├── Shard Key: '{}'", shard);
            }
            if !statement.embed.is_empty() {
                let _ = writeln!(output, "└── Embed Directives: {}", statement.embed.len());
            } else {
                output.push_str("└── Status: direct payload\n");
            }
        }
        Stmt::CreateCollection(statement) => {
            let mode = match statement.mode {
                CollectionMode::Dense { .. } => "dense",
                CollectionMode::Hybrid { .. } => "hybrid",
                CollectionMode::Rerank => "rerank-oriented",
            };
            output.push_str("Statement: CREATE COLLECTION\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            let _ = writeln!(output, "├── Mode: {}", mode);
            let _ = writeln!(
                output,
                "└── Vectors: {} dense, {} sparse",
                statement.vectors.len(),
                statement.sparse_vectors.len()
            );
        }
        Stmt::CreateIndex(statement) => {
            output.push_str("Statement: CREATE INDEX\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            let _ = writeln!(output, "└── Field: {}", statement.field);
        }
        Stmt::CreateShardKey(statement) => {
            output.push_str("Statement: CREATE SHARD KEY\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            let _ = writeln!(output, "└── Shard: {}", statement.shard_key);
        }
        Stmt::DropShardKey(statement) => {
            output.push_str("Statement: DROP SHARD KEY\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            let _ = writeln!(output, "└── Shard: {}", statement.shard_key);
        }
        Stmt::ShowShardKeys(collection) => {
            let _ = writeln!(output, "Statement: SHOW SHARD KEYS [{}]", collection);
        }
        Stmt::ShowQuotas => {
            output.push_str("Statement: SHOW QUOTAS\n");
        }
        Stmt::SetQuota(statement) => {
            output.push_str("Statement: SET QUOTA\n");
            for (key, value) in &statement.config {
                let _ = writeln!(output, "  {} = {}", key, render_quota_value(value));
            }
        }
        Stmt::DropIndex(statement) => {
            output.push_str("Statement: DROP INDEX\n");
            let _ = writeln!(output, "├── Collection: {}", statement.collection);
            let _ = writeln!(output, "└── Field: {}", statement.field);
        }
        Stmt::Count(statement) => {
            output.push_str("Statement: COUNT\n");
            let col = match &statement.collection {
                QueryCollection::Explicit(c) => c.as_str(),
                QueryCollection::Inherited => "inherited",
            };
            let _ = writeln!(output, "├── Collection: {}", col);
            if let Some(f) = &statement.filter {
                let _ = writeln!(output, "├── Filter: {}", render_filter(f));
            }
            let _ = writeln!(output, "└── Exact: {}", statement.exact.unwrap_or(false));
        }
        Stmt::Facet(statement) => {
            output.push_str("Statement: FACET\n");
            let col = match &statement.collection {
                QueryCollection::Explicit(c) => c.as_str(),
                QueryCollection::Inherited => "inherited",
            };
            let _ = writeln!(output, "├── Key: {}", statement.key);
            let _ = writeln!(output, "├── Collection: {}", col);
            if let Some(f) = &statement.filter {
                let _ = writeln!(output, "├── Filter: {}", render_filter(f));
            }
            if let Some(l) = statement.limit {
                let _ = writeln!(output, "├── Limit: {}", l);
            }
            let _ = writeln!(output, "└── Exact: {}", statement.exact.unwrap_or(false));
        }
        Stmt::AlterCollection(statement) => {
            let _ = writeln!(
                output,
                "Statement: ALTER COLLECTION [{}]",
                statement.collection
            );
        }
        Stmt::DropCollection(statement) => {
            let _ = writeln!(
                output,
                "Statement: DROP COLLECTION [{}]",
                statement.collection
            );
        }
        Stmt::ShowCollections => {
            output.push_str("Statement: SHOW COLLECTIONS\n");
        }
        Stmt::ShowCollection(collection) => {
            let _ = writeln!(output, "Statement: SHOW COLLECTION [{}]", collection);
        }
        Stmt::Delete(statement) => {
            let _ = writeln!(output, "Statement: DELETE FROM {}", statement.collection);
        }
        Stmt::ClearPayload(statement) => {
            let _ = writeln!(
                output,
                "Statement: CLEAR PAYLOAD ON {}",
                statement.collection
            );
        }
        Stmt::DeletePayload(statement) => {
            let _ = writeln!(
                output,
                "Statement: DELETE PAYLOAD ({:?}) ON {}",
                statement.keys, statement.collection
            );
        }
        Stmt::DeleteVector(statement) => {
            let _ = writeln!(
                output,
                "Statement: DELETE VECTOR ({:?}) ON {}",
                statement.vector_names, statement.collection
            );
        }
        Stmt::UpdateVector(statement) => {
            let _ = writeln!(
                output,
                "Statement: UPDATE VECTOR ON {}",
                statement.collection
            );
        }
        Stmt::UpdatePayload(statement) => {
            let _ = writeln!(
                output,
                "Statement: UPDATE PAYLOAD ON {}",
                statement.collection
            );
        }
    }
    output
}

fn query_target_vector(expression: &QueryExpr) -> Option<String> {
    match expression {
        QueryExpr::Nearest { using, .. }
        | QueryExpr::Recommend { using, .. }
        | QueryExpr::Context { using, .. }
        | QueryExpr::Discover { using, .. } => using.as_ref().map(|vt| {
            let kind = vt.kind.map(|k| format!(" as {:?}", k)).unwrap_or_default();
            format!("{}{}", vt.name, kind)
        }),
        QueryExpr::Hybrid {
            dense_vector,
            sparse_vector,
            fusion,
            ..
        } => {
            let d = dense_vector.as_deref().unwrap_or("dense");
            let s = sparse_vector.as_deref().unwrap_or("sparse");
            let f = match fusion {
                FusionMethod::Rrf => "RRF",
                FusionMethod::Dbsf => "DBSF",
            };
            Some(format!("dense='{}', sparse='{}', fusion={}", d, s, f))
        }
        _ => None,
    }
}

fn query_intent(expression: &QueryExpr) -> &'static str {
    match expression {
        QueryExpr::Points { .. } => "retrieve points by ID",
        QueryExpr::Nearest { mmr: Some(_), .. } => "maximal marginal relevance (MMR) search",
        QueryExpr::Nearest { input, .. } => match input {
            QueryInput::Text { .. } => "nearest neighbors from text",
            QueryInput::Image { .. } => "nearest neighbors from an image",
            QueryInput::Vector(_) => "nearest neighbors from a vector",
            QueryInput::Point(_) => "nearest neighbors from a point",
            QueryInput::Param(_) | QueryInput::PositionalParam(_) => {
                "nearest neighbors from query parameter"
            }
        },
        QueryExpr::Recommend { .. } => "recommend from positive and negative examples",
        QueryExpr::Context { .. } => "context search",
        QueryExpr::Discover { .. } => "discovery search",
        QueryExpr::OrderBy { .. } => "payload order query",
        QueryExpr::SampleRandom => "random sample",
        QueryExpr::Fusion { .. } => "fuse prefetched result sets",
        QueryExpr::Formula { .. } => "formula-based scoring",
        QueryExpr::RelevanceFeedback { .. } => "relevance feedback",
        QueryExpr::Hybrid { .. } => "hybrid shorthand (dense + sparse)",
        QueryExpr::Rerank { .. } => "late-interaction prefetched rerank",
        QueryExpr::CrossRerank { .. } => "cross-encoder pair rerank of prefetched candidates",
    }
}

fn query_prefetches(expression: &QueryExpr) -> Option<Vec<String>> {
    match expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => Some(
            prefetch
                .iter()
                .map(|p| match &p.source {
                    PrefetchSource::Cte(name) => format!("CTE '{}'", name),
                    PrefetchSource::Query(q) => {
                        format!("inline query: {}", query_intent(&q.expression))
                    }
                })
                .collect(),
        ),
        _ => None,
    }
}

fn render_quota_value(value: &Value) -> String {
    match value {
        Value::Str(s) => format!("'{}'", s),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Dict(_) => "<object>".into(),
        Value::List(_) => "<list>".into(),
        Value::Param(name) => format!(":{}", name),
        Value::PositionalParam(_) => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explain_tree_structure() {
        let q = "QUERY TEXT 'chest pain' FROM medical USING dense WHERE department = 'cardio' SHARD 'east' LIMIT 5;";
        let plan = explain(q).unwrap();
        assert!(plan.contains("Statement: QUERY [nearest neighbors from text]"));
        assert!(plan.contains("├── Collection: medical"));
        assert!(plan.contains("├── Target Vector: dense"));
        assert!(plan.contains("├── Shard Key: 'east'"));
        assert!(plan.contains("├── Filter: department = 'cardio'"));
        assert!(plan.contains("└── Pagination: limit=5, offset=0"));
    }
}
