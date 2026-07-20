use crate::ast::{
    DeleteStmt, FilterExpr, QueryStmt, ScrollStmt, Stmt, UpdatePayloadStmt, UpsertStmt, Value,
};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;

/// Merges an existing filter expression with a new filter expression using AND.
fn merge_filters(
    existing: Option<Box<FilterExpr>>,
    new_filter: FilterExpr,
) -> Option<Box<FilterExpr>> {
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
fn build_filter(field: String, op: String, value: Value) -> FilterExpr {
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
pub fn inject_query_filter(q: &mut QueryStmt, field: String, op: String, value: Value) {
    let new_filter = build_filter(field.clone(), op.clone(), value.clone());
    q.query_filter = merge_filters(q.query_filter.take(), new_filter);

    for cte in &mut q.ctes {
        inject_query_filter(&mut cte.stmt, field.clone(), op.clone(), value.clone());
    }
}

/// Injects a filter into a ScrollStmt.
pub fn inject_scroll_filter(s: &mut ScrollStmt, field: String, op: String, value: Value) {
    let new_filter = build_filter(field, op, value);
    s.query_filter = merge_filters(s.query_filter.take(), new_filter);
}

/// Injects a filter into a DeleteStmt.
pub fn inject_delete_filter(d: &mut DeleteStmt, field: String, op: String, value: Value) {
    let new_filter = build_filter(field, op, value);
    d.query_filter = merge_filters(d.query_filter.take(), new_filter);
}

/// Injects a filter into an UpdatePayloadStmt.
pub fn inject_update_payload_filter(
    u: &mut UpdatePayloadStmt,
    field: String,
    op: String,
    value: Value,
) {
    let new_filter = build_filter(field, op, value);
    u.query_filter = merge_filters(u.query_filter.take(), new_filter);
}

/// Forces a field value into every UPSERT payload row.
///
/// This is deliberately limited to equality-style tenant stamping. Other
/// operators describe predicates, not payload mutations, so they are ignored
/// for UPSERT rather than inventing ambiguous row semantics.
pub fn inject_upsert_value(i: &mut UpsertStmt, field: &str, op: &str, value: &Value) {
    if op != "=" {
        return;
    }

    for row in &mut i.values_list {
        if let Some((_, existing)) = row.iter_mut().rev().find(|(k, _)| *k == field) {
            *existing = value.clone();
        } else {
            row.push((field.to_string(), value.clone()));
        }
    }
}

/// Forces a field value into a Value::Dict payload.
pub fn inject_dict_value(dict: &mut Value, field: String, value: Value) {
    dict.dict_set(field, value);
}

/// Injects a filter condition recursively into the WHERE clause of the given Stmt.
pub fn inject_filter(stmt: &mut Stmt, field: &str, op: &str, value: &Value) {
    match stmt {
        Stmt::Query(ref mut q) => {
            inject_query_filter(q, field.to_string(), op.to_string(), value.clone())
        }
        Stmt::Scroll(ref mut s) => {
            inject_scroll_filter(s, field.to_string(), op.to_string(), value.clone())
        }
        Stmt::Delete(ref mut d) => {
            inject_delete_filter(d, field.to_string(), op.to_string(), value.clone())
        }
        Stmt::UpdatePayload(ref mut u) => {
            inject_update_payload_filter(u, field.to_string(), op.to_string(), value.clone())
        }
        Stmt::Upsert(ref mut i) => inject_upsert_value(i, field, op, value),
        _ => {}
    }
}
