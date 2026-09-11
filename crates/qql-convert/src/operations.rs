//! Per-operation converters: upsert, search, recommend, discover,
//! scroll, point lookups, deletes, payload writes.

use serde_json::Value;

use crate::ConvertError;
use crate::filter::convert_filter_to_qql;
use crate::formatters::{
    build_payload_dict, escape_qql_string, format_vector, lookup_from_str, using_str,
};
use crate::sanitize::{format_id, format_id_list};

/// Convert an upsert body to one `UPSERT INTO` per point.
pub(crate) fn convert_upsert(input: &Value, collection: &str) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid upsert JSON")),
    };
    let points = match obj.get("points").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Err(ConvertError::InvalidPayload("no points in upsert payload")),
    };

    let mut stmts = Vec::new();
    for point in points {
        let pobj = match point.as_object() {
            Some(o) => o,
            None => continue,
        };

        let mut payload = serde_json::Map::new();
        if let Some(p) = pobj.get("payload").and_then(|v| v.as_object()) {
            for (k, v) in p {
                payload.insert(k.clone(), v.clone());
            }
        }
        if let Some(id) = pobj.get("id") {
            payload.insert("id".to_string(), id.clone());
        }
        if let Some(vec) = pobj.get("vector") {
            payload.insert("vector".to_string(), vec.clone());
        } else if let Some(vectors) = pobj.get("vectors").and_then(|v| v.as_object()) {
            let mut vmap = serde_json::Map::new();
            for (k, v) in vectors {
                vmap.insert(k.clone(), v.clone());
            }
            payload.insert("vector".to_string(), Value::Object(vmap));
        }

        let values = build_payload_dict(&payload);
        stmts.push(format!("UPSERT INTO {collection} VALUES {values}"));
    }

    if stmts.is_empty() {
        return Err(ConvertError::InvalidPayload(
            "no points found in upsert payload",
        ));
    }
    Ok(stmts)
}

/// Convert a search body to `QUERY ... FROM ...`.
pub(crate) fn convert_search(input: &Value, collection: &str) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid search JSON")),
    };

    let mut parts = Vec::new();
    let mut qql_args = Vec::new();

    // Determine query text/vector/ID
    let query_raw = obj.get("query");
    let vector = obj.get("vector");

    if let Some(q) = query_raw {
        if let Some(s) = q.as_str() {
            qql_args.push(format!("QUERY '{}'", escape_qql_string(s)));
        } else if let Some(_arr) = q.as_array() {
            let vec_str = format_vector(q).unwrap_or_default();
            qql_args.push(format!("QUERY {vec_str}"));
        } else if let Some(qobj) = q.as_object() {
            if let Some(text) = qobj.get("text").and_then(|v| v.as_str()) {
                if let Some(model) = qobj.get("model").and_then(|v| v.as_str()) {
                    qql_args.push(format!(
                        "QUERY '{}' USING MODEL '{}'",
                        escape_qql_string(text),
                        escape_qql_string(model)
                    ));
                } else {
                    qql_args.push(format!("QUERY '{}'", escape_qql_string(text)));
                }
            } else if let Some(sample) = qobj.get("sample") {
                if sample.is_object() {
                    qql_args.push("QUERY SAMPLE".to_string());
                }
            } else if qobj.contains_key("indices") {
                qql_args.push("QUERY '<sparse_query>' USING SPARSE".to_string());
            } else {
                qql_args.push("QUERY '<query>'".to_string());
            }
        }
    } else if let Some(v) = vector {
        if let Some(_arr) = v.as_array() {
            let vec_str = format_vector(v).unwrap_or_default();
            qql_args.push(format!("QUERY {vec_str}"));
        }
    } else {
        qql_args.push("QUERY '<query>'".to_string());
    }

    qql_args.push(format!("FROM {collection}"));

    // Limit
    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        qql_args.push(format!("LIMIT {limit}"));
    }

    // Offset
    if let Some(offset) = obj.get("offset").and_then(|v| v.as_i64()) {
        qql_args.push(format!("OFFSET {offset}"));
    }

    // Using
    if let Some(using) = obj.get("using").and_then(|v| v.as_str()) {
        qql_args.push(using_str(using));
    }

    // Filter
    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            qql_args.push(format!("WHERE {filter_str}"));
        }
    }

    // Score threshold
    if let Some(threshold) = obj.get("score_threshold").and_then(|v| v.as_f64()) {
        qql_args.push(format!("SCORE THRESHOLD {threshold}"));
    }

    // Group by
    if let Some(group_by) = obj.get("group_by").and_then(|v| v.as_str()) {
        qql_args.push(format!("GROUP BY '{}'", escape_qql_string(group_by)));
        if let Some(group_size) = obj.get("group_size").and_then(|v| v.as_i64()) {
            qql_args.push(format!("GROUP_SIZE {group_size}"));
        }
    }

    // Lookup from
    if let Some(lookup) = obj.get("lookup_from").and_then(|v| v.as_object())
        && let Some(s) = lookup_from_str(lookup)
    {
        qql_args.push(s);
    }

    parts.push(qql_args.join(" "));
    Ok(parts)
}

/// Convert a recommend body to `QUERY RECOMMEND`.
pub(crate) fn convert_recommend(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid recommend JSON")),
    };

    let (positive, negative, strategy) = extract_recommend_params(obj);

    let positive_str = format_id_list(&positive);
    let mut with_parts = vec![format!("positive = ({positive_str})")];
    if !negative.is_empty() {
        let negative_str = format_id_list(&negative);
        with_parts.push(format!("negative = ({negative_str})"));
    }

    let mut parts = vec![format!(
        "QUERY RECOMMEND WITH ({}) FROM {}",
        with_parts.join(", "),
        collection
    )];

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        parts.push(format!("LIMIT {limit}"));
    }

    if let Some(s) = strategy {
        parts.push(format!("STRATEGY '{}'", escape_qql_string(&s)));
    }

    if let Some(using) = obj.get("using").and_then(|v| v.as_str()) {
        parts.push(using_str(using));
    }

    if let Some(lookup) = obj.get("lookup_from").and_then(|v| v.as_object())
        && let Some(s) = lookup_from_str(lookup)
    {
        parts.push(s);
    }

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            parts.push(format!("WHERE {filter_str}"));
        }
    }

    Ok(vec![parts.join(" ")])
}

fn extract_recommend_params(
    obj: &serde_json::Map<String, Value>,
) -> (Vec<Value>, Vec<Value>, Option<String>) {
    // Check top-level
    let mut positive = obj
        .get("positive")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut negative = obj
        .get("negative")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut strategy = obj
        .get("strategy")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check nested query.recommend
    if let Some(query) = obj.get("query").and_then(|v| v.as_object())
        && let Some(recommend) = query.get("recommend").and_then(|v| v.as_object())
    {
        if positive.is_empty() {
            positive = recommend
                .get("positive")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
        }
        if negative.is_empty() {
            negative = recommend
                .get("negative")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
        }
        if strategy.is_none() {
            strategy = recommend
                .get("strategy")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }

    (positive, negative, strategy)
}

/// Convert a discover body to `QUERY DISCOVER` / `QUERY CONTEXT`.
pub(crate) fn convert_discover(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid discover JSON")),
    };

    let (target, ctx_pairs) = extract_discover_params(obj);

    let mut parts = Vec::new();
    if let Some(t) = target {
        parts.push(format!("QUERY DISCOVER TARGET {}", format_id(&t)));
        if !ctx_pairs.is_empty() {
            let pairs: Vec<String> = ctx_pairs
                .iter()
                .map(|(p, n)| format!("({}, {})", format_id(p), format_id(n)))
                .collect();
            parts.push(format!("CONTEXT PAIRS {}", pairs.join(", ")));
        }
    } else {
        parts.push("QUERY CONTEXT".to_string());
        if !ctx_pairs.is_empty() {
            let pairs: Vec<String> = ctx_pairs
                .iter()
                .map(|(p, n)| format!("({}, {})", format_id(p), format_id(n)))
                .collect();
            parts.push(format!("PAIRS {}", pairs.join(", ")));
        }
    }

    parts.push(format!("FROM {collection}"));

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        parts.push(format!("LIMIT {limit}"));
    }

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            parts.push(format!("WHERE {filter_str}"));
        }
    }

    Ok(vec![parts.join(" ")])
}

fn extract_discover_params(
    obj: &serde_json::Map<String, Value>,
) -> (Option<Value>, Vec<(Value, Value)>) {
    let target = obj.get("target").cloned();
    let ctx_pairs = extract_context_pairs(obj);

    // Check nested query.discover
    if let Some(query) = obj.get("query").and_then(|v| v.as_object()) {
        if let Some(discover) = query.get("discover").and_then(|v| v.as_object()) {
            let nested_target = discover.get("target").cloned();
            let nested_pairs = if let Some(ctx) = discover.get("context").and_then(|v| v.as_array())
            {
                ctx.iter()
                    .filter_map(|pair| {
                        pair.as_object().and_then(|p| {
                            let pos = p.get("positive")?.clone();
                            let neg = p.get("negative")?.clone();
                            Some((pos, neg))
                        })
                    })
                    .collect()
            } else {
                vec![]
            };
            if nested_target.is_some() || !nested_pairs.is_empty() {
                return (nested_target.or(target), nested_pairs);
            }
        }
        // Check query.context
        if let Some(ctx) = query.get("context").and_then(|v| v.as_array()) {
            let qpairs: Vec<(Value, Value)> = ctx
                .iter()
                .filter_map(|pair| {
                    pair.as_object().and_then(|p| {
                        let pos = p.get("positive")?.clone();
                        let neg = p.get("negative")?.clone();
                        Some((pos, neg))
                    })
                })
                .collect();
            if !qpairs.is_empty() {
                return (None, qpairs);
            }
        }
    }

    (target, ctx_pairs)
}

fn extract_context_pairs(obj: &serde_json::Map<String, Value>) -> Vec<(Value, Value)> {
    if let Some(ctx) = obj.get("context").and_then(|v| v.as_array()) {
        ctx.iter()
            .filter_map(|pair| {
                pair.as_object().and_then(|p| {
                    let pos = p.get("positive")?.clone();
                    let neg = p.get("negative")?.clone();
                    Some((pos, neg))
                })
            })
            .collect()
    } else {
        vec![]
    }
}

/// Convert a scroll body to `SCROLL FROM`.
pub(crate) fn convert_scroll(input: &Value, collection: &str) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid scroll JSON")),
    };

    let mut parts = vec![format!("SCROLL FROM {collection}")];

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            parts.push(format!("WHERE {filter_str}"));
        }
    }

    if let Some(after) = obj.get("offset") {
        parts.push(format!("AFTER {}", format_id(after)));
    }

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        parts.push(format!("LIMIT {limit}"));
    }

    Ok(vec![parts.join(" ")])
}

/// Convert a get-points body to one `QUERY POINTS` per ID.
pub(crate) fn convert_get_points(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid get points JSON")),
    };
    let ids = match obj.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Err(ConvertError::InvalidPayload("no ids in get points payload")),
    };

    let stmts: Vec<String> = ids
        .iter()
        .map(|id| format!("QUERY POINTS ({}) FROM {}", format_id(id), collection))
        .collect();

    Ok(stmts)
}

/// Convert a delete body (by IDs or by filter) to `DELETE FROM`.
pub(crate) fn convert_delete_points(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid delete JSON")),
    };

    if let Some(_filter) = obj.get("filter") {
        return convert_delete_by_filter(input, collection);
    }

    let points = match obj.get("points").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Err(ConvertError::InvalidPayload("no points in delete payload")),
    };

    let stmts: Vec<String> = points
        .iter()
        .map(|id| format!("DELETE FROM {collection} WHERE id = {}", format_id(id)))
        .collect();
    Ok(stmts)
}

/// Convert a filter-only delete body to `DELETE FROM ... WHERE ...`.
pub(crate) fn convert_delete_by_filter(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid delete filter JSON")),
    };

    let filter = match obj.get("filter") {
        Some(f) => f,
        None => {
            return Err(ConvertError::InvalidPayload(
                "no filter in delete by filter payload",
            ));
        }
    };

    let filter_str = convert_filter_to_qql(filter)?;
    Ok(vec![format!("DELETE FROM {collection} WHERE {filter_str}")])
}

/// Convert a set-payload body to `UPDATE ... SET PAYLOAD`.
pub(crate) fn convert_set_payload(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid set payload JSON")),
    };

    let payload = match obj.get("payload").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => {
            return Err(ConvertError::InvalidPayload(
                "no payload in set payload JSON",
            ));
        }
    };

    let payload_str = build_payload_dict(payload);

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        return Ok(vec![format!(
            "UPDATE {collection} SET PAYLOAD = {payload_str} WHERE {filter_str}"
        )]);
    }

    if let Some(points) = obj.get("points").and_then(|v| v.as_array()) {
        let stmts: Vec<String> = points
            .iter()
            .map(|id| {
                format!(
                    "UPDATE {collection} SET PAYLOAD = {payload_str} WHERE id = {}",
                    format_id(id)
                )
            })
            .collect();
        return Ok(stmts);
    }

    Err(ConvertError::InvalidPayload(
        "set payload requires points or filter",
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{convert_get_points, convert_upsert};

    #[test]
    fn upsert_conversion_emits_canonical_qql() {
        let statements = convert_upsert(
            &json!({
                "points": [{
                    "id": 1,
                    "vector": [0.1, 0.2],
                    "payload": {"title": "hello"}
                }]
            }),
            "docs",
        )
        .expect("upsert conversion");

        assert_eq!(statements.len(), 1);
        assert!(statements[0].starts_with("UPSERT INTO docs VALUES "));
    }

    #[test]
    fn point_lookup_conversion_emits_canonical_qql() {
        let statements =
            convert_get_points(&json!({"ids": [1, "point-2"]}), "docs").expect("point conversion");

        assert_eq!(
            statements,
            [
                "QUERY POINTS (1) FROM docs",
                "QUERY POINTS ('point-2') FROM docs"
            ]
        );
    }
}
