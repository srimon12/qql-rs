use crate::ast::{self, Stmt};
use crate::error::QqlError;
use crate::parser::Parser;

/// Parse and explain a QQL query — pure AST formatting, no server needed.
pub fn explain(query: &str) -> Result<String, QqlError> {
    let stmt = Parser::parse(query)?;
    explain_node(&stmt)
}

/// Format an already-parsed AST node into a human-readable plan.
pub fn explain_node(stmt: &Stmt) -> Result<String, QqlError> {
    let mut plan = String::new();
    explain_stmt(stmt, &mut plan);
    plan.push_str("Action: Explain-only mode (no Qdrant server)\n");
    Ok(plan)
}

fn explain_stmt(stmt: &Stmt, plan: &mut String) {
    match stmt {
        Stmt::ShowCollections => {
            plan.push_str("Statement: SHOW COLLECTIONS\n");
        }
        Stmt::ShowCollection(collection) => {
            plan.push_str(&format!("Statement: SHOW COLLECTION {}\n", collection));
        }
        Stmt::CreateCollection(s) => {
            plan.push_str(&format!("Statement: CREATE COLLECTION {}\n", s.collection));
            if let Some(model) = &s.model {
                plan.push_str(&format!("Model: {}\n", model));
            }
            if s.rerank {
                plan.push_str("Type: HYBRID + RERANK (dense + sparse + ColBERT multivector)\n");
            } else if s.hybrid {
                plan.push_str("Type: HYBRID (dense + sparse)\n");
            } else {
                plan.push_str("Type: DENSE\n");
            }
            for v in &s.vectors {
                plan.push_str(&format!("Vector: {}, Size: {}\n", v.name, v.size));
            }
        }
        Stmt::AlterCollection(s) => {
            plan.push_str(&format!("Statement: ALTER COLLECTION {}\n", s.collection));
        }
        Stmt::DropCollection(s) => {
            plan.push_str(&format!("Statement: DROP COLLECTION {}\n", s.collection));
        }
        Stmt::Upsert(s) => {
            plan.push_str(&format!("Statement: UPSERT INTO {}\n", s.collection));
            if let Some(model) = &s.model {
                plan.push_str(&format!("Model: {}\n", model));
            }
            plan.push_str(&format!("Rows: {}\n", s.values_list.len()));
        }
        Stmt::Select(s) => {
            plan.push_str(&format!(
                "Statement: SELECT * FROM {} WHERE id = '{:?}'\n",
                s.collection, s.point_id
            ));
        }
        Stmt::Scroll(s) => {
            plan.push_str(&format!(
                "Statement: SCROLL FROM {} LIMIT {}\n",
                s.collection, s.limit
            ));
        }
        Stmt::Query(q) => {
            let mode_str = match q.mode {
                ast::QueryMode::Nearest => "NEAREST",
                ast::QueryMode::Recommend => "RECOMMEND",
                ast::QueryMode::Context => "CONTEXT",
                ast::QueryMode::Discover => "DISCOVER",
                ast::QueryMode::OrderBy => "ORDER BY",
                ast::QueryMode::Sample => "SAMPLE",
                ast::QueryMode::RelevanceFeedback => "RELEVANCE FEEDBACK",
            };
            let coll: &str = q.collection.as_deref().unwrap_or("<none>");
            if !mode_str.is_empty() {
                plan.push_str(&format!(
                    "Statement: QUERY {} FROM {} LIMIT {}\n",
                    mode_str, coll, q.limit
                ));
            } else {
                plan.push_str(&format!(
                    "Statement: QUERY FROM {} LIMIT {}\n",
                    coll, q.limit
                ));
            }
            if let Some(text) = &q.query_text {
                plan.push_str(&format!("Query: '{}'\n", text));
            }
            if !q.raw_vector.is_empty() {
                plan.push_str(&format!("Raw Vector: {:?}\n", q.raw_vector));
            }
            match q.query_type {
                ast::QueryType::Hybrid => plan.push_str("Using: HYBRID\n"),
                ast::QueryType::Sparse => plan.push_str("Using: SPARSE\n"),
                ast::QueryType::Dense => {}
            }
            if let Some(u) = &q.using_ {
                plan.push_str(&format!("Using: '{}'\n", u));
            }
            if let Some(m) = &q.model {
                plan.push_str(&format!("Model: {}\n", m));
            }
            if q.offset > 0 {
                plan.push_str(&format!("Offset: {}\n", q.offset));
            }
            if let Some(th) = &q.score_threshold {
                plan.push_str(&format!("Score threshold: {}\n", th));
            }
            if let Some(gb) = &q.group_by {
                plan.push_str(&format!("Group by: {}\n", gb));
            }
            if q.rerank {
                plan.push_str("Rerank: enabled\n");
            }
            if !q.ctes.is_empty() {
                plan.push_str(&format!("CTEs: {} defined\n", q.ctes.len()));
            }
            if !q.prefetch_refs.is_empty() {
                plan.push_str(&format!("Prefetch refs: {}\n", q.prefetch_refs.len()));
            }
            if let Some(ft) = &q.fusion_type {
                plan.push_str(&format!("Fusion: {}\n", ft));
            }
        }
        Stmt::Delete(s) => {
            if let Some(field) = &s.field {
                plan.push_str(&format!(
                    "Statement: DELETE FROM {} WHERE {} = '{:?}'\n",
                    s.collection, field, s.value
                ));
            } else {
                plan.push_str(&format!(
                    "Statement: DELETE FROM {} WHERE id = '{:?}'\n",
                    s.collection, s.point_id
                ));
            }
        }
        Stmt::UpdateVector(s) => {
            plan.push_str(&format!(
                "Statement: UPDATE {} SET VECTOR = [...] WHERE id = '{:?}'\n",
                s.collection, s.point_id
            ));
        }
        Stmt::UpdatePayload(s) => {
            plan.push_str(&format!(
                "Statement: UPDATE {} SET PAYLOAD = {{...}} WHERE id = '{:?}'\n",
                s.collection, s.point_id
            ));
        }
        Stmt::CreateIndex(s) => {
            plan.push_str(&format!(
                "Statement: CREATE INDEX ON COLLECTION {} FOR {} TYPE {}\n",
                s.collection, s.field, s.field_type
            ));
        }
    }
}
