use crate::ast::{
    DeleteStmt, FilterExpr, InsertStmt, QueryStmt, ScrollStmt, Stmt, UpdatePayloadStmt, Value,
};
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec;

/// Merges an existing filter expression with a new filter expression using AND.
fn merge_filters<'a>(
    existing: Option<Box<FilterExpr<'a>>>,
    new_filter: FilterExpr<'a>,
) -> Option<Box<FilterExpr<'a>>> {
    match existing {
        Some(expr) => match *expr {
            FilterExpr::And { mut operands } => {
                operands.push(new_filter);
                Some(Box::new(FilterExpr::And { operands }))
            }
            other => Some(Box::new(FilterExpr::And {
                operands: vec![other, new_filter],
            })),
        },
        None => Some(Box::new(new_filter)),
    }
}

/// Builds a new FilterExpr from a field, operator, and value.
fn build_filter<'a>(field: &'a str, op: &'a str, value: Value<'a>) -> FilterExpr<'a> {
    match op.to_lowercase().as_str() {
        "in" => {
            if let Value::List(vals) = value {
                FilterExpr::In {
                    field,
                    values: vals,
                }
            } else {
                FilterExpr::In {
                    field,
                    values: vec![value],
                }
            }
        }
        "not_in" | "not in" => {
            if let Value::List(vals) = value {
                FilterExpr::NotIn {
                    field,
                    values: vals,
                }
            } else {
                FilterExpr::NotIn {
                    field,
                    values: vec![value],
                }
            }
        }
        _ => FilterExpr::Compare { field, op, value },
    }
}

/// Recursively injects a filter into a QueryStmt and all of its nested CTE prefetch statements.
pub fn inject_query_filter<'a>(
    q: &mut QueryStmt<'a>,
    field: &'a str,
    op: &'a str,
    value: &Value<'a>,
) {
    let new_filter = build_filter(field, op, value.clone());
    q.query_filter = merge_filters(q.query_filter.take(), new_filter);

    for cte in &mut q.ctes {
        inject_query_filter(&mut cte.stmt, field, op, value);
    }
}

/// Injects a filter into a ScrollStmt.
pub fn inject_scroll_filter<'a>(
    s: &mut ScrollStmt<'a>,
    field: &'a str,
    op: &'a str,
    value: &Value<'a>,
) {
    let new_filter = build_filter(field, op, value.clone());
    s.query_filter = merge_filters(s.query_filter.take(), new_filter);
}

/// Injects a filter into a DeleteStmt.
pub fn inject_delete_filter<'a>(
    d: &mut DeleteStmt<'a>,
    field: &'a str,
    op: &'a str,
    value: &Value<'a>,
) {
    let new_filter = build_filter(field, op, value.clone());
    d.query_filter = merge_filters(d.query_filter.take(), new_filter);
}

/// Injects a filter into an UpdatePayloadStmt.
pub fn inject_update_payload_filter<'a>(
    u: &mut UpdatePayloadStmt<'a>,
    field: &'a str,
    op: &'a str,
    value: &Value<'a>,
) {
    let new_filter = build_filter(field, op, value.clone());
    u.query_filter = merge_filters(u.query_filter.take(), new_filter);
}

/// Forces a field value into every INSERT payload row.
///
/// This is deliberately limited to equality-style tenant stamping. Other
/// operators describe predicates, not payload mutations, so they are ignored
/// for INSERT rather than inventing ambiguous row semantics.
pub fn inject_insert_value<'a>(
    i: &mut InsertStmt<'a>,
    field: &'a str,
    op: &'a str,
    value: &Value<'a>,
) {
    if op != "=" {
        return;
    }

    for row in &mut i.values_list {
        if let Some((_, existing)) = row.iter_mut().rev().find(|(k, _)| *k == field) {
            *existing = value.clone();
        } else {
            row.push((field, value.clone()));
        }
    }
}

/// Forces a field value into a Value::Dict payload.
pub fn inject_dict_value<'a>(dict: &mut Value<'a>, field: &'a str, value: &Value<'a>) {
    dict.dict_set(Cow::Borrowed(field), value.clone());
}

/// Injects a filter condition recursively into the WHERE clause of the given Stmt.
pub fn inject_filter<'a>(stmt: &mut Stmt<'a>, field: &'a str, op: &'a str, value: &Value<'a>) {
    match stmt {
        Stmt::Query(ref mut q) => inject_query_filter(q, field, op, value),
        Stmt::Scroll(ref mut s) => inject_scroll_filter(s, field, op, value),
        Stmt::Delete(ref mut d) => inject_delete_filter(d, field, op, value),
        Stmt::UpdatePayload(ref mut u) => inject_update_payload_filter(u, field, op, value),
        Stmt::Insert(ref mut i) => inject_insert_value(i, field, op, value),
        _ => {}
    }
}
