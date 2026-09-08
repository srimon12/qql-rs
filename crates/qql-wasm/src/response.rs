//! REST response shaping: topology names plus typed hit envelopes.

use super::report::exec_response;

/// Extract named dense/sparse/multivector names from a Qdrant collection `result` object.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn vector_names_from_collection_result(
    result: &serde_json::Value,
) -> qql_embed::TopologyNames {
    let params = result.get("config").and_then(|c| c.get("params"));
    let mut dense = Vec::new();
    let mut multivector = Vec::new();
    if let Some(vectors) = params
        .and_then(|p| p.get("vectors"))
        .and_then(|v| v.as_object())
    {
        if vectors.contains_key("size") && vectors.contains_key("distance") {
            // Unnamed default dense vector.
            dense.clear();
        } else {
            for (name, cfg) in vectors {
                if matches!(
                    name.as_str(),
                    "size"
                        | "distance"
                        | "hnsw_config"
                        | "quantization_config"
                        | "on_disk"
                        | "multivector_config"
                ) {
                    continue;
                }
                dense.push(name.clone());
                if cfg.get("multivector_config").is_some() {
                    multivector.push(name.clone());
                }
            }
            dense.sort();
            multivector.sort();
        }
    }
    let mut sparse = Vec::new();
    if let Some(map) = params
        .and_then(|p| p.get("sparse_vectors"))
        .and_then(|v| v.as_object())
    {
        sparse.extend(map.keys().cloned());
        sparse.sort();
    }
    qql_embed::TopologyNames {
        dense,
        sparse,
        multivector,
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn wasm_search_hits(result: &serde_json::Value) -> serde_json::Value {
    let points = result
        .get("result")
        .and_then(|value| value.get("points"))
        .and_then(serde_json::Value::as_array)
        .or_else(|| result.get("points").and_then(serde_json::Value::as_array))
        .or_else(|| result.get("result").and_then(serde_json::Value::as_array));

    serde_json::Value::Array(
        points
            .into_iter()
            .flatten()
            .map(|hit| {
                let id = hit
                    .get("id")
                    .map(|id| match id {
                        serde_json::Value::String(value) => value.clone(),
                        serde_json::Value::Number(value) => value.to_string(),
                        _ => id.to_string(),
                    })
                    .unwrap_or_default();
                let payload = hit
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let text = payload
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "id": id,
                    "score": hit.get("score").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                    "text": text,
                    "payload": payload,
                })
            })
            .collect(),
    )
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn wasm_success_response(
    operation: &qql_plan::PlannedOperation,
    result: serde_json::Value,
) -> serde_json::Value {
    use qql_plan::PlannedOperation;

    let label = operation.operation_label();
    let (message, data) = match operation {
        PlannedOperation::Query { .. }
        | PlannedOperation::Scroll { .. }
        | PlannedOperation::GetPoints { .. } => {
            let hits = wasm_search_hits(&result);
            let count = hits.as_array().map_or(0, Vec::len);
            (format!("Found {count} hits"), Some(hits))
        }
        PlannedOperation::QueryGroups { .. } => {
            let count = result
                .get("result")
                .and_then(|value| value.get("groups"))
                .or_else(|| result.get("groups"))
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            (format!("Found {count} group(s)"), Some(result))
        }
        PlannedOperation::Count { .. } => {
            let count = result
                .get("result")
                .and_then(|value| value.get("count"))
                .or_else(|| result.get("count"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            (format!("Count: {count}"), Some(result))
        }
        PlannedOperation::Facet { .. } => {
            let facet_hits = result
                .get("result")
                .and_then(|value| value.get("hits"))
                .cloned()
                .or_else(|| result.get("hits").cloned())
                .unwrap_or_else(|| serde_json::json!([]));
            let hits = facet_hits.as_array().map_or(0, Vec::len);
            (format!("Found {hits} facet hit(s)"), Some(facet_hits))
        }
        PlannedOperation::Upsert { request, .. } => (
            format!("Upserted {} point(s)", request.points.len()),
            Some(serde_json::json!({"count": request.points.len()})),
        ),
        PlannedOperation::ListShardKeys { .. } => ("Shard keys listed".to_string(), Some(result)),
        _ => (format!("{label} ok"), None),
    };
    exec_response(true, label, &message, data)
}
