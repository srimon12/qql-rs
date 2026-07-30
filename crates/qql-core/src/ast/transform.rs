use super::{
    ComparisonOp, FilterExpr, PointId, PointIdPredicate, PointSelector, Prefetch, PrefetchSource,
    QueryExpr, QueryStmt, Stmt, Value,
};
use crate::error::QqlError;
use alloc::boxed::Box;
use alloc::string::{String, ToString};

impl Stmt {
    /// Custom shard routing key for this statement, if any.
    ///
    /// Corresponds to QQL `SHARD '…'` on DML, lowered to request-level
    /// `shard_key` (REST) / `ShardKeySelector` (gRPC) — never inside `Filter`.
    pub fn shard_key(&self) -> Option<&str> {
        match self {
            Self::Query(query) => query.shard_key.as_deref(),
            Self::Scroll(scroll) => scroll.shard_key.as_deref(),
            Self::Count(count) => count.shard_key.as_deref(),
            Self::Upsert(upsert) => upsert.shard_key.as_deref(),
            Self::Delete(delete) => delete.shard_key.as_deref(),
            Self::ClearPayload(clear) => clear.shard_key.as_deref(),
            Self::DeletePayload(delete) => delete.shard_key.as_deref(),
            Self::DeleteVector(delete) => delete.shard_key.as_deref(),
            Self::UpdateVector(update) => update.shard_key.as_deref(),
            Self::UpdatePayload(update) => update.shard_key.as_deref(),
            _ => None,
        }
    }

    /// Set custom shard routing (same field as QQL `SHARD '…'`).
    ///
    /// Prefer writing `SHARD 'tenant'` in the query when the tenant is known at
    /// authoring time. Use this setter only when the host resolves the key after
    /// parse (e.g. from auth context) without re-stringifying QQL.
    ///
    /// On `QUERY`, recurses into CTEs and nested prefetch queries so routing
    /// matches a top-level `SHARD` clause. Empty / `None` clears the key.
    /// Returns `false` for statement types that cannot carry routing (DDL, SHOW).
    pub fn set_shard_key(&mut self, shard_key: Option<String>) -> bool {
        let key = shard_key.filter(|k| !k.is_empty());
        match self {
            Self::Query(query) => {
                apply_query_shard(query, key.as_deref());
                true
            }
            Self::Scroll(scroll) => {
                scroll.shard_key = key;
                true
            }
            Self::Count(count) => {
                count.shard_key = key;
                true
            }
            Self::Upsert(upsert) => {
                upsert.shard_key = key;
                true
            }
            Self::Delete(delete) => {
                delete.shard_key = key;
                true
            }
            Self::ClearPayload(clear) => {
                clear.shard_key = key;
                true
            }
            Self::DeletePayload(delete) => {
                delete.shard_key = key;
                true
            }
            Self::DeleteVector(delete) => {
                delete.shard_key = key;
                true
            }
            Self::UpdateVector(update) => {
                update.shard_key = key;
                true
            }
            Self::UpdatePayload(update) => {
                update.shard_key = key;
                true
            }
            _ => false,
        }
    }
}

/// Apply shard routing to a query and nested CTE / prefetch queries.
fn apply_query_shard(query: &mut QueryStmt, key: Option<&str>) {
    query.shard_key = key.map(str::to_string);
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
        Stmt::DeletePayload(del) => merge_selector(&mut del.selector, filter),
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
        other => {
            return Err(QqlError::validation(
                "QQL-VALIDATION-FILTER-INJECT",
                format!(
                    "inject_filter does not apply to this statement type ({})",
                    stmt_kind(other)
                ),
                None,
            ));
        }
    }
    Ok(())
}

fn stmt_kind(statement: &Stmt) -> &'static str {
    match statement {
        Stmt::Query(_) => "QUERY",
        Stmt::Scroll(_) => "SCROLL",
        Stmt::Count(_) => "COUNT",
        Stmt::Upsert(_) => "UPSERT",
        Stmt::Delete(_) => "DELETE",
        Stmt::ClearPayload(_) => "CLEAR PAYLOAD",
        Stmt::DeletePayload(_) => "DELETE PAYLOAD",
        Stmt::DeleteVector(_) => "DELETE VECTOR",
        Stmt::UpdateVector(_) => "UPDATE VECTOR",
        Stmt::UpdatePayload(_) => "UPDATE PAYLOAD",
        Stmt::CreateCollection(_) => "CREATE COLLECTION",
        Stmt::AlterCollection(_) => "ALTER COLLECTION",
        Stmt::DropCollection(_) => "DROP COLLECTION",
        Stmt::CreateIndex(_) => "CREATE INDEX",
        Stmt::DropIndex(_) => "DROP INDEX",
        Stmt::CreateShardKey(_) => "CREATE SHARD KEY",
        Stmt::DropShardKey(_) => "DROP SHARD KEY",
        Stmt::ShowCollections => "SHOW COLLECTIONS",
        Stmt::ShowCollection(_) => "SHOW COLLECTION",
        Stmt::ShowShardKeys(_) => "SHOW SHARD KEYS",
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
