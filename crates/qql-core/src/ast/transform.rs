use super::{
    ComparisonOp, FilterExpr, PointId, PointIdPredicate, PointSelector, Prefetch, PrefetchSource,
    QueryExpr, QueryStmt, ShardKey, Stmt, Value,
};
use crate::error::QqlError;
use alloc::boxed::Box;
use alloc::string::ToString;

impl Stmt {
    /// Custom shard routing key for this statement, if any.
    ///
    /// Corresponds to QQL `SHARD` on DML, lowered to request-level
    /// `shard_key` (REST) / `ShardKeySelector` (gRPC) — never inside `Filter`.
    /// Keyword and numeric forms are preserved (`ShardKey::Number(101)` reads
    /// back as a number, never coerced to `"101"`).
    pub fn shard_key(&self) -> Option<&ShardKey> {
        match self {
            Self::Query(query) => query.shard_key.as_ref(),
            Self::Scroll(scroll) => scroll.shard_key.as_ref(),
            Self::Count(count) => count.shard_key.as_ref(),
            Self::Facet(facet) => facet.shard_key.as_ref(),
            Self::Upsert(upsert) => upsert.shard_key.as_ref(),
            Self::Delete(delete) => delete.shard_key.as_ref(),
            Self::ClearPayload(clear) => clear.shard_key.as_ref(),
            Self::DeletePayload(delete) => delete.shard_key.as_ref(),
            Self::DeleteVector(delete) => delete.shard_key.as_ref(),
            Self::UpdateVector(update) => update.shard_key.as_ref(),
            Self::UpdatePayload(update) => update.shard_key.as_ref(),
            _ => None,
        }
    }

    /// Set custom shard routing (same field as QQL `SHARD`).
    ///
    /// Prefer writing the `SHARD` clause in the query when the tenant is known
    /// at authoring time. Use this setter only when the host resolves the key
    /// after parse (e.g. from auth context) without re-stringifying QQL.
    ///
    /// On `QUERY`, recurses into CTEs and nested prefetch queries so routing
    /// matches a top-level `SHARD` clause. `None` (or an empty keyword) clears
    /// the key.
    /// Returns `false` for statement types that cannot carry routing (DDL, SHOW).
    pub fn set_shard_key(&mut self, shard_key: Option<ShardKey>) -> bool {
        let shard_key = shard_key.filter(|k| !matches!(k, ShardKey::Keyword(s) if s.is_empty()));
        match self {
            Self::Query(query) => {
                apply_query_shard(query, shard_key.as_ref());
                true
            }
            Self::Scroll(scroll) => {
                scroll.shard_key = shard_key;
                true
            }
            Self::Count(count) => {
                count.shard_key = shard_key;
                true
            }
            Self::Facet(facet) => {
                facet.shard_key = shard_key;
                true
            }
            Self::Upsert(upsert) => {
                upsert.shard_key = shard_key;
                true
            }
            Self::Delete(delete) => {
                delete.shard_key = shard_key;
                true
            }
            Self::ClearPayload(clear) => {
                clear.shard_key = shard_key;
                true
            }
            Self::DeletePayload(delete) => {
                delete.shard_key = shard_key;
                true
            }
            Self::DeleteVector(delete) => {
                delete.shard_key = shard_key;
                true
            }
            Self::UpdateVector(update) => {
                update.shard_key = shard_key;
                true
            }
            Self::UpdatePayload(update) => {
                update.shard_key = shard_key;
                true
            }
            _ => false,
        }
    }
}

/// Apply shard routing to a query and nested CTE / prefetch queries.
fn apply_query_shard(query: &mut QueryStmt, key: Option<&ShardKey>) {
    query.shard_key = key.cloned();
    for cte in &mut query.ctes {
        apply_query_shard(&mut cte.query, key);
    }
    if let Some(prefetches) = expression_prefetch(&mut query.expression) {
        for prefetch in prefetches {
            if let PrefetchSource::Query(nested) = &mut prefetch.source {
                apply_query_shard(nested, key);
            }
        }
    }
}

/// Injects a typed field comparison into a statement (CTEs and prefetches included), fail-closed.
pub fn inject_filter(
    statement: &mut Stmt,
    field: &str,
    operator: ComparisonOp,
    value: Value,
) -> Result<(), QqlError> {
    let filter = build_filter(field, operator, value.clone())?;
    match statement {
        Stmt::Query(query) => inject_query(query, &filter),
        Stmt::Scroll(scroll) => merge_filter(&mut scroll.filter, filter),
        Stmt::Delete(delete) => merge_selector(&mut delete.selector, filter),
        Stmt::Count(count) => merge_filter(&mut count.filter, filter),
        Stmt::Facet(facet) => merge_filter(&mut facet.filter, filter),
        Stmt::ClearPayload(clear) => merge_selector(&mut clear.selector, filter),
        Stmt::DeletePayload(del) => merge_selector(&mut del.selector, filter),
        Stmt::DeleteVector(del_vec) => merge_selector(&mut del_vec.selector, filter),
        Stmt::UpdatePayload(update) => merge_selector(&mut update.selector, filter),
        Stmt::Upsert(_) if operator != ComparisonOp::Eq || field.eq_ignore_ascii_case("id") => {
            return Err(QqlError::validation(
                "QQL-VALIDATION-FILTER-INJECT",
                "inject_filter into UPSERT requires Eq on a non-id payload field",
                None,
            ));
        }
        Stmt::Upsert(upsert) => {
            for point in &mut upsert.points {
                // A whole-point placeholder has no payload yet — silently
                // skipping it would drop a security filter. Bind first.
                let inline = match point {
                    crate::ast::PointEntry::Inline(inline) => inline,
                    crate::ast::PointEntry::Param(name, _) => {
                        return Err(QqlError::validation(
                            "QQL-VALIDATION-FILTER-INJECT",
                            alloc::format!(
                                "cannot inject filter into unbound point parameter ':{name}'; bind point parameters before filter injection"
                            ),
                            None,
                        ));
                    }
                    crate::ast::PointEntry::PositionalParam(idx, _) => {
                        return Err(QqlError::validation(
                            "QQL-VALIDATION-FILTER-INJECT",
                            alloc::format!(
                                "cannot inject filter into unbound point parameter '?{}'; bind point parameters before filter injection",
                                *idx + 1
                            ),
                            None,
                        ));
                    }
                };
                if let Some((_, current)) = inline
                    .payload
                    .iter_mut()
                    .find(|(key, _)| key.eq_ignore_ascii_case(field))
                {
                    *current = value.clone();
                } else {
                    inline.payload.push((field.to_string(), value.clone()));
                }
            }
        }
        other => {
            return Err(QqlError::validation(
                "QQL-VALIDATION-FILTER-INJECT",
                format!(
                    "inject_filter does not apply to this statement type ({})",
                    other.stmt_kind()
                ),
                None,
            ));
        }
    }
    Ok(())
}

fn build_filter(field: &str, operator: ComparisonOp, value: Value) -> Result<FilterExpr, QqlError> {
    if field.eq_ignore_ascii_case("id") {
        if operator != ComparisonOp::Eq {
            return Err(QqlError::validation(
                "QQL-VALIDATION-ID-PREDICATE",
                "point ID injection supports equality only",
                None,
            ));
        }
        let id = match value {
            Value::Int(value) if value >= 0 => PointId::Number(value as u64),
            Value::Str(value) => PointId::String(value),
            _ => {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-POINT-ID",
                    "point IDs must be unsigned integers or strings",
                    None,
                ));
            }
        };
        Ok(FilterExpr::PointId(PointIdPredicate::Eq(id)))
    } else {
        Ok(FilterExpr::Compare {
            field: field.to_string(),
            op: operator,
            value,
        })
    }
}

fn inject_query(query: &mut QueryStmt, filter: &FilterExpr) {
    merge_filter(&mut query.filter, filter.clone());
    for cte in &mut query.ctes {
        inject_query(&mut cte.query, filter);
    }
    if let Some(prefetches) = expression_prefetch(&mut query.expression) {
        for prefetch in prefetches {
            merge_filter(&mut prefetch.filter, filter.clone());
            if let PrefetchSource::Query(query) = &mut prefetch.source {
                inject_query(query, filter);
            }
        }
    }
}

fn expression_prefetch(expression: &mut QueryExpr) -> Option<&mut Vec<Prefetch>> {
    match expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => Some(prefetch),
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => None,
    }
}

fn merge_selector(selector: &mut PointSelector, filter: FilterExpr) {
    let current = match core::mem::replace(selector, PointSelector::Ids(Vec::new())) {
        PointSelector::Id(id) => FilterExpr::PointId(PointIdPredicate::Eq(id)),
        PointSelector::Ids(ids) => FilterExpr::PointId(PointIdPredicate::In(ids)),
        PointSelector::Filter(existing) => *existing,
    };
    *selector = PointSelector::Filter(Box::new(and(current, filter)));
}

fn merge_filter(current: &mut Option<Box<FilterExpr>>, filter: FilterExpr) {
    *current = Some(Box::new(match current.take() {
        Some(current) => and(*current, filter),
        None => filter,
    }));
}

fn and(left: FilterExpr, right: FilterExpr) -> FilterExpr {
    match left {
        FilterExpr::And { mut operands } => {
            operands.push(right);
            FilterExpr::And { operands }
        }
        left => FilterExpr::And {
            operands: alloc::vec![left, right],
        },
    }
}
