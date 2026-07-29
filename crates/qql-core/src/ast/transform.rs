use super::{
    ComparisonOp, FilterExpr, PointId, PointIdPredicate, PointSelector, Prefetch, PrefetchSource,
    QueryExpr, QueryStmt, Stmt, Value,
};
use crate::error::QqlError;
use alloc::boxed::Box;
use alloc::string::ToString;

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
        Stmt::ClearPayload(clear) => merge_selector(&mut clear.selector, filter),
        Stmt::DeleteVector(del_vec) => merge_selector(&mut del_vec.selector, filter),
        Stmt::UpdatePayload(update) => merge_selector(&mut update.selector, filter),
        Stmt::Upsert(upsert)
            if operator == ComparisonOp::Eq && !field.eq_ignore_ascii_case("id") =>
        {
            for point in &mut upsert.points {
                if let Some((_, current)) = point
                    .payload
                    .iter_mut()
                    .find(|(key, _)| key.eq_ignore_ascii_case(field))
                {
                    *current = value.clone();
                } else {
                    point.payload.push((field.to_string(), value.clone()));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Inject a custom shard key for multi-tenant routing (host-side, no language bind params).
///
/// Qdrant accepts `shard_key` on query/mutation bodies (OpenAPI / proto
/// `ShardKeySelector`). QQL has no `$param` shard syntax — multi-tenant hosts
/// call this after resolving the tenant id (or set `shard_key` on the AST).
///
/// Applies to statements that carry `shard_key` in the AST: `QUERY`, `SCROLL`,
/// `COUNT`, `UPSERT`, `DELETE`, `CLEAR PAYLOAD`, `DELETE VECTOR`,
/// `UPDATE … VECTOR`, and `UPDATE … PAYLOAD`. Recurses into CTEs and nested
/// prefetch queries. No-op for DDL.
pub fn inject_shard_key(statement: &mut Stmt, shard_key: &str) -> Result<(), QqlError> {
    if shard_key.is_empty() {
        return Err(QqlError::validation(
            "QQL-VALIDATION-SHARD-KEY",
            "shard key must be a non-empty string",
            None,
        ));
    }
    let key = shard_key.to_string();
    match statement {
        Stmt::Query(query) => inject_query_shard(query, &key),
        Stmt::Scroll(scroll) => scroll.shard_key = Some(key),
        Stmt::Count(count) => count.shard_key = Some(key),
        Stmt::Upsert(upsert) => upsert.shard_key = Some(key),
        Stmt::Delete(delete) => delete.shard_key = Some(key),
        Stmt::ClearPayload(clear) => clear.shard_key = Some(key),
        Stmt::DeleteVector(delete) => delete.shard_key = Some(key),
        Stmt::UpdateVector(update) => update.shard_key = Some(key),
        Stmt::UpdatePayload(update) => update.shard_key = Some(key),
        _ => {}
    }
    Ok(())
}

fn inject_query_shard(query: &mut QueryStmt, key: &str) {
    query.shard_key = Some(key.to_string());
    for cte in &mut query.ctes {
        inject_query_shard(&mut cte.query, key);
    }
    if let Some(prefetches) = expression_prefetch(&mut query.expression) {
        for prefetch in prefetches {
            if let PrefetchSource::Query(nested) = &mut prefetch.source {
                inject_query_shard(nested, key);
            }
        }
    }
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
    let current =
        match core::mem::replace(selector, PointSelector::Filter(Box::new(filter.clone()))) {
            PointSelector::Id(id) => FilterExpr::PointId(PointIdPredicate::Eq(id)),
            PointSelector::Ids(ids) => FilterExpr::PointId(PointIdPredicate::In(ids)),
            PointSelector::Filter(filter) => *filter,
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
