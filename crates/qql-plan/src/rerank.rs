//! Client-side CROSS RERANK planning: candidate queries + payload field ensure.

use crate::plan::PlannedOperation;
use qql_core::error::QqlError;

pub(crate) fn plan_cross_rerank(
    outer: &qql_core::ast::QueryStmt,
    ctes: &[qql_core::ast::Cte],
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
        // The candidate body is planned standalone (its collection is made
        // explicit below), under the CTE scope visible where the prefetch was
        // written: the enclosing scope for inline sources, the prefix before
        // the named CTE for references.
        let (mut sub, sub_ctes): (qql_core::ast::QueryStmt, &[qql_core::ast::Cte]) =
            match &pref.source {
                PrefetchSource::Cte(name) => {
                    let Some(index) = ctes.iter().position(|c| c.name.eq_ignore_ascii_case(name))
                    else {
                        return Err(QqlError::validation(
                            "QQL-PLAN-CROSS-RERANK-CTE",
                            format!("PREFETCH references unknown CTE '{name}'"),
                            None,
                        ));
                    };
                    ((*ctes[index].query).clone(), &ctes[..index])
                }
                PrefetchSource::Query(q) => ((**q).clone(), ctes),
            };
        if matches!(sub.collection, QueryCollection::Inherited) {
            sub.collection = QueryCollection::Explicit(collection.to_string());
        }
        // Candidate stage needs document text for pair scoring.
        ensure_payload_field(&mut sub, &field);
        if let Some(f) = &pref.filter {
            sub.filter = Some(f.clone());
        }
        // Candidate stages are planned through the same lowering as any query,
        // minus the unbound-param gate: the outer statement already ran it
        // (the param census recurses into CROSS RERANK prefetch sources), and
        // template planning (`plan_template`) must leave vector placeholders
        // for `bind_vector_params` to fill at execution time.
        let planned = crate::plan::lower_query_to_planned(&sub, sub_ctes)?;
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
