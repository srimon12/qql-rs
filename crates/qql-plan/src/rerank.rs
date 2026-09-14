//! Client-side CROSS RERANK planning: candidate queries + payload field ensure.

use crate::plan::{PlannedOperation, plan};
use qql_core::ast::Stmt;
use qql_core::error::QqlError;

pub(crate) fn plan_cross_rerank(
    outer: &qql_core::ast::QueryStmt,
    collection: &str,
    query_text: &str,
    model: &str,
    field: &Option<String>,
    prefetch: &[qql_core::ast::Prefetch],
) -> Result<PlannedOperation, QqlError> {
    use qql_core::ast::{PrefetchSource, QueryCollection};

    if prefetch.is_empty() {
        return Err(QqlError::validation(
            "QQL-PLAN-CROSS-RERANK-PREFETCH",
            "CROSS RERANK requires at least one PREFETCH",
            None,
        ));
    }
    if query_text.is_empty() {
        return Err(QqlError::validation(
            "QQL-PLAN-CROSS-RERANK-QUERY",
            "CROSS RERANK query text must not be empty",
            None,
        ));
    }
    if model.is_empty() {
        return Err(QqlError::validation(
            "QQL-PLAN-CROSS-RERANK-MODEL",
            "CROSS RERANK MODEL must not be empty",
            None,
        ));
    }
    let field = field
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("text")
        .to_string();

    let mut candidates = Vec::with_capacity(prefetch.len());
    for pref in prefetch {
        let mut sub = match &pref.source {
            PrefetchSource::Cte(name) => {
                let cte = outer
                    .ctes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(name));
                let Some(cte) = cte else {
                    return Err(QqlError::validation(
                        "QQL-PLAN-CROSS-RERANK-CTE",
                        format!("PREFETCH references unknown CTE '{name}'"),
                        None,
                    ));
                };
                (*cte.query).clone()
            }
            PrefetchSource::Query(q) => (**q).clone(),
        };
        if matches!(sub.collection, QueryCollection::Inherited) {
            sub.collection = QueryCollection::Explicit(collection.to_string());
        }
        // Candidate stage needs document text for pair scoring.
        ensure_payload_field(&mut sub, &field);
        if let Some(f) = &pref.filter {
            sub.filter = Some(f.clone());
        }
        let planned = plan(&Stmt::Query(Box::new(sub)))?;
        match planned {
            PlannedOperation::Query {
                collection: c,
                request,
            } => candidates.push((c, request)),
            other => {
                return Err(QqlError::validation(
                    "QQL-PLAN-CROSS-RERANK-CANDIDATE",
                    format!(
                        "CROSS RERANK prefetch must plan as a search query, got {}",
                        other.operation_label()
                    ),
                    None,
                ));
            }
        }
    }

    Ok(PlannedOperation::CrossRerank {
        collection: collection.to_string(),
        query: query_text.to_string(),
        model: model.to_string(),
        field,
        limit: outer.page.limit.unwrap_or(10),
        offset: outer.page.offset.unwrap_or(0),
        candidates,
    })
}

fn ensure_payload_field(query: &mut qql_core::ast::QueryStmt, field: &str) {
    use qql_core::ast::{PayloadSelector, QueryOutput};
    match &mut query.output {
        QueryOutput {
            payload: None | Some(PayloadSelector::None),
            ..
        } => {
            query.output.payload = Some(PayloadSelector::Include(vec![field.to_string()]));
        }
        QueryOutput {
            payload: Some(PayloadSelector::Include(fields)),
            ..
        } => {
            if !fields.iter().any(|f| f.eq_ignore_ascii_case(field)) {
                fields.push(field.to_string());
            }
        }
        QueryOutput {
            payload: Some(PayloadSelector::All | PayloadSelector::Exclude(_)),
            ..
        } => {}
    }
}
