//! Formula-query and prefetch-CTE conversion.

use serde_json::Value;

use crate::ConvertError;
use crate::detect::convert_by_structure;
use crate::filter::convert_filter_to_qql;
use crate::formatters::{escape_qql_string, format_vector, using_str};

/// Convert a `/points/query` body (formula, nearest, fusion, batch) to QQL.
pub(crate) fn convert_formula_query(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid formula query JSON")),
    };

    if let Some(searches) = obj.get("searches").and_then(|v| v.as_array()) {
        let mut all_stmts = Vec::new();
        for search in searches {
            let sub = convert_by_structure(search, collection)?;
            all_stmts.extend(sub);
        }
        return Ok(all_stmts);
    }

    let mut stmt_parts = Vec::new();
    stmts_with_prefetch(obj, collection, &mut stmt_parts, 0)?;

    // Base query part
    if let Some(query) = obj.get("query").and_then(|v| v.as_object()) {
        if let Some(text) = query.get("text").and_then(|v| v.as_str()) {
            stmt_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
        } else if let Some(nearest) = query.get("nearest") {
            if let Some(arr) = nearest.as_array() {
                let vec_str = format_vector(&Value::Array(arr.clone())).unwrap_or_default();
                stmt_parts.push(format!("QUERY {vec_str}"));
            } else if let Some(doc_map) = nearest.as_object() {
                if let Some(text) = doc_map.get("text").and_then(|v| v.as_str()) {
                    stmt_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
                } else {
                    stmt_parts.push("QUERY '<query>'".to_string());
                }
            } else {
                stmt_parts.push("QUERY '<query>'".to_string());
            }
        } else if query.contains_key("fusion") {
            // Fusion-only query
            let fusion = query
                .get("fusion")
                .and_then(|v| v.as_str())
                .unwrap_or("rrf");
            stmt_parts.push(format!("FUSION {}", fusion.to_uppercase()));
        }
    } else {
        stmt_parts.push("QUERY '<query>'".to_string());
    }

    stmt_parts.push(format!("FROM {collection}"));

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        stmt_parts.push(format!("LIMIT {limit}"));
    }

    if let Some(offset) = obj.get("offset").and_then(|v| v.as_i64()) {
        stmt_parts.push(format!("OFFSET {offset}"));
    }

    // Filter
    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            stmt_parts.push(format!("WHERE {filter_str}"));
        }
    }

    // Using
    if let Some(using) = obj.get("using").and_then(|v| v.as_str()) {
        stmt_parts.push(using_str(using));
    }

    // Score Threshold
    if let Some(threshold) = obj.get("score_threshold").and_then(|v| v.as_f64()) {
        stmt_parts.push(format!("SCORE THRESHOLD {threshold}"));
    }

    Ok(vec![stmt_parts.join(" ")])
}

/// Lower nested `prefetch` arrays into `WITH ... AS (...)` CTEs plus a
/// `PREFETCH (...)` reference list, prepended to `stmt_parts`.
fn stmts_with_prefetch(
    obj: &serde_json::Map<String, Value>,
    _collection: &str,
    stmt_parts: &mut Vec<String>,
    depth: usize,
) -> Result<(), ConvertError> {
    if let Some(prefetch) = obj.get("prefetch") {
        let prefetches = match prefetch {
            Value::Array(arr) => arr.clone(),
            Value::Object(_) => vec![prefetch.clone()],
            _ => return Ok(()),
        };

        for (i, pf) in prefetches.iter().enumerate() {
            let cte_name = format!("_pf{}", depth * 100 + i);
            let pf_obj = match pf.as_object() {
                Some(o) => o,
                None => continue,
            };

            let mut pf_parts = Vec::new();
            stmts_with_prefetch(pf_obj, _collection, &mut pf_parts, depth + 1)?;

            if let Some(query) = pf_obj.get("query") {
                if let Some(qobj) = query.as_object() {
                    if let Some(text) = qobj.get("text").and_then(|v| v.as_str()) {
                        pf_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
                    }
                } else if let Some(s) = query.as_str() {
                    pf_parts.push(format!("QUERY '{}'", escape_qql_string(s)));
                }
            } else if let Some(document) = pf_obj.get("document").and_then(|v| v.as_object()) {
                if let Some(text) = document.get("text").and_then(|v| v.as_str()) {
                    pf_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
                }
            } else if let Some(vector) = pf_obj.get("vector").and_then(|v| v.as_array())
                && !vector.is_empty()
            {
                let vs: Vec<String> = vector.iter().map(|v| v.to_string()).collect();
                pf_parts.push(format!("QUERY [{}]", vs.join(", ")));
            }

            if let Some(limit) = pf_obj.get("limit").and_then(|v| v.as_i64()) {
                pf_parts.push(format!("LIMIT {limit}"));
            }

            if let Some(filter) = pf_obj.get("filter") {
                let filter_str = convert_filter_to_qql(filter)?;
                if !filter_str.is_empty() {
                    pf_parts.push(format!("WHERE {filter_str}"));
                }
            }

            if let Some(threshold) = pf_obj.get("score_threshold").and_then(|v| v.as_f64()) {
                pf_parts.push(format!("SCORE THRESHOLD {threshold}"));
            }

            if let Some(using) = pf_obj.get("using").and_then(|v| v.as_str()) {
                pf_parts.push(using_str(using));
            }

            if pf_parts.is_empty() {
                pf_parts.push("QUERY ''".to_string());
            }

            let cte_str = pf_parts.join(" ");
            stmt_parts.push(format!("WITH {cte_name} AS ({cte_str})"));
        }

        let refs: Vec<String> = (0..prefetches.len())
            .map(|i| format!("_pf{}", depth * 100 + i))
            .collect();
        stmt_parts.push(format!("PREFETCH ({})", refs.join(", ")));
    }

    Ok(())
}
