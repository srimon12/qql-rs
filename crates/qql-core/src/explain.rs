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
        output.push_str(&format!("── Statement {} ──\n", i + 1));
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
            output.push_str(&format!("Statement: QUERY [{}]\n", intent));

            let col = match &query.collection {
                QueryCollection::Explicit(collection) => collection.as_str(),
                QueryCollection::Inherited => "inherited",
            };
            output.push_str(&format!("├── Collection: {}\n", col));

            if let Some(target) = query_target_vector(&query.expression) {
                output.push_str(&format!("├── Target Vector: {}\n", target));
            }

            if let Some(shard) = &query.shard_key {
                output.push_str(&format!("├── Shard Key: '{}'\n", shard));
            }

            if !query.ctes.is_empty() {
                output.push_str(&format!("├── CTEs ({}):\n", query.ctes.len()));
                for (i, cte) in query.ctes.iter().enumerate() {
                    let is_last = i + 1 == query.ctes.len();
                    let prefix = if is_last {
                        "│   └──"
                    } else {
                        "│   ├──"
                    };
                    output.push_str(&format!(
                        "{} '{}': {}\n",
                        prefix,
                        cte.name,
                        query_intent(&cte.query.expression)
                    ));
                }
            }

            if let Some(prefetches) = query_prefetches(&query.expression) {
                if !prefetches.is_empty() {
                    output.push_str(&format!("├── Prefetches ({}):\n", prefetches.len()));
                    for (i, pf) in prefetches.iter().enumerate() {
                        let is_last = i + 1 == prefetches.len();
                        let prefix = if is_last {
                            "│   └──"
                        } else {
                            "│   ├──"
                        };
                        output.push_str(&format!("{} [{}] {}\n", prefix, i + 1, pf));
                    }
                }
            }

            if let Some(filter) = &query.filter {
                output.push_str(&format!("├── Filter: {}\n", render_filter(filter)));
            }

            if let Some(params) = &query.params {
                output.push_str(&format!(
                    "├── Search Params: {}\n",
                    render_search_params(params)
                ));
            }

            if let Some(score) = query.score_threshold {
                output.push_str(&format!("├── Score Threshold: {}\n", score));
            }

            if let Some(group) = &query.group {
                let size_str = group
                    .size
                    .map(|s| format!(" (size: {})", s))
                    .unwrap_or_default();
                output.push_str(&format!("├── Group By: {}{}\n", group.field, size_str));
            }

            if let Some(payload) = &query.output.payload {
                match payload {
                    PayloadSelector::Include(keys) => {
                        output.push_str(&format!("├── Payload: INCLUDE ({})\n", keys.join(", ")));
                    }
                    PayloadSelector::Exclude(keys) => {
                        output.push_str(&format!("├── Payload: EXCLUDE ({})\n", keys.join(", ")));
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
                        output.push_str(&format!("├── Vectors: ({})\n", names.join(", ")));
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
            output.push_str(&format!(
                "└── Pagination: limit={}, offset={}\n",
                limit, offset
            ));
        }
        Stmt::Scroll(statement) => {
            output.push_str("Statement: SCROLL\n");
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            if let Some(f) = &statement.filter {
                output.push_str(&format!("├── Filter: {}\n", render_filter(f)));
            }
            if let Some(shard) = &statement.shard_key {
                output.push_str(&format!("├── Shard Key: '{}'\n", shard));
            }
            output.push_str(&format!("└── Limit: {}\n", statement.limit));
        }
        Stmt::Upsert(statement) => {
            output.push_str("Statement: UPSERT\n");
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            output.push_str(&format!("├── Points: {}\n", statement.points.len()));
            if let Some(shard) = &statement.shard_key {
                output.push_str(&format!("├── Shard Key: '{}'\n", shard));
            }
            if !statement.embed.is_empty() {
                output.push_str(&format!(
                    "└── Embed Directives: {}\n",
                    statement.embed.len()
                ));
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
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            output.push_str(&format!("├── Mode: {}\n", mode));
            output.push_str(&format!(
                "└── Vectors: {} dense, {} sparse\n",
                statement.vectors.len(),
                statement.sparse_vectors.len()
            ));
        }
        Stmt::CreateIndex(statement) => {
            output.push_str("Statement: CREATE INDEX\n");
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            output.push_str(&format!("└── Field: {}\n", statement.field));
        }
        Stmt::CreateShardKey(statement) => {
            output.push_str("Statement: CREATE SHARD KEY\n");
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            output.push_str(&format!("└── Shard: {}\n", statement.shard_key));
        }
        Stmt::DropShardKey(statement) => {
            output.push_str("Statement: DROP SHARD KEY\n");
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            output.push_str(&format!("└── Shard: {}\n", statement.shard_key));
        }
        Stmt::ShowShardKeys(collection) => {
            output.push_str(&format!("Statement: SHOW SHARD KEYS [{}]\n", collection));
        }
        Stmt::ShowQuotas => {
            output.push_str("Statement: SHOW QUOTAS\n");
        }
        Stmt::SetQuota(statement) => {
            output.push_str("Statement: SET QUOTA\n");
            for (key, value) in &statement.config {
                output.push_str(&format!("  {} = {}\n", key, render_quota_value(value)));
            }
        }
        Stmt::DropIndex(statement) => {
            output.push_str("Statement: DROP INDEX\n");
            output.push_str(&format!("├── Collection: {}\n", statement.collection));
            output.push_str(&format!("└── Field: {}\n", statement.field));
        }
        Stmt::Count(statement) => {
            output.push_str("Statement: COUNT\n");
            let col = match &statement.collection {
                QueryCollection::Explicit(c) => c.as_str(),
                QueryCollection::Inherited => "inherited",
            };
            output.push_str(&format!("├── Collection: {}\n", col));
            if let Some(f) = &statement.filter {
                output.push_str(&format!("├── Filter: {}\n", render_filter(f)));
            }
            output.push_str(&format!(
                "└── Exact: {}\n",
                statement.exact.unwrap_or(false)
            ));
        }
        Stmt::AlterCollection(statement) => {
            output.push_str(&format!(
                "Statement: ALTER COLLECTION [{}]\n",
                statement.collection
            ));
        }
        Stmt::DropCollection(statement) => {
            output.push_str(&format!(
                "Statement: DROP COLLECTION [{}]\n",
                statement.collection
            ));
        }
        Stmt::ShowCollections => {
            output.push_str("Statement: SHOW COLLECTIONS\n");
        }
        Stmt::ShowCollection(collection) => {
            output.push_str(&format!("Statement: SHOW COLLECTION [{}]\n", collection));
        }
        Stmt::Delete(statement) => {
            output.push_str(&format!(
                "Statement: DELETE FROM {}\n",
                statement.collection
            ));
        }
        Stmt::ClearPayload(statement) => {
            output.push_str(&format!(
                "Statement: CLEAR PAYLOAD ON {}\n",
                statement.collection
            ));
        }
        Stmt::DeletePayload(statement) => {
            output.push_str(&format!(
                "Statement: DELETE PAYLOAD ({:?}) ON {}\n",
                statement.keys, statement.collection
            ));
        }
        Stmt::DeleteVector(statement) => {
            output.push_str(&format!(
                "Statement: DELETE VECTOR ({:?}) ON {}\n",
                statement.vector_names, statement.collection
            ));
        }
        Stmt::UpdateVector(statement) => {
            output.push_str(&format!(
                "Statement: UPDATE VECTOR ON {}\n",
                statement.collection
            ));
        }
        Stmt::UpdatePayload(statement) => {
            output.push_str(&format!(
                "Statement: UPDATE PAYLOAD ON {}\n",
                statement.collection
            ));
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
