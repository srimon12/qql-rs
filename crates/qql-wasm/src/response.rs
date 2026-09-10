//! Strict REST response shaping for the WASM JS boundary.
//!
//! WASM cannot depend on `qql-runtime`, so it re-shapes Qdrant REST results
//! into the same JSON that the runtime's closed `ExecData` enum serializes.
//! Extraction is per-operation and strict: each operation reads exactly its
//! OpenAPI response field, with no envelope fallback chain and no synthesized
//! fields (in particular, hits carry no `text`).

use serde_json::{Map, Value};

use super::report::exec_response_with_telemetry;
use super::telemetry::telemetry_from_envelope;

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

/// One canonical hit: `{id, score, payload, collection?, vector?}` — the JSON
/// shape of the runtime's `SearchHit`. IDs keep their JSON type (number or
/// string) and `score` is rounded through `f32` like the typed pipeline.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn ranked_hit(point: &Value) -> Value {
    let mut hit = Map::new();
    hit.insert("id".into(), point.get("id").cloned().unwrap_or(Value::Null));
    let score = point.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    hit.insert(
        "score".into(),
        score
            .to_string()
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::from(score as f64)),
    );
    hit.insert(
        "payload".into(),
        point.get("payload").cloned().unwrap_or(Value::Null),
    );
    if let Some(vector) = point.get("vector") {
        hit.insert("vector".into(), vector.clone());
    }
    Value::Object(hit)
}

/// Canonical hit array from an optional point list.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn hit_array(points: Option<&Value>) -> Vec<Value> {
    points
        .and_then(Value::as_array)
        .map(|points| points.iter().map(ranked_hit).collect())
        .unwrap_or_default()
}

/// `result.points` — the `/points/query` and `/points/scroll` envelope.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_points(result: &Value) -> Option<&Value> {
    result.get("result")?.get("points")
}

/// `result` — the bare array envelope of `POST /points` (retrieve).
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_records(result: &Value) -> Option<&Value> {
    result.get("result")
}

/// `result.groups` — the grouped query envelope, shaped as canonical
/// `[{"id": …, "hits": [...]}]`.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_groups(result: &Value) -> Vec<Value> {
    result
        .get("result")
        .and_then(|result| result.get("groups"))
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .map(|group| {
                    let mut shaped = Map::new();
                    shaped.insert("id".into(), group.get("id").cloned().unwrap_or(Value::Null));
                    shaped.insert("hits".into(), Value::Array(hit_array(group.get("hits"))));
                    Value::Object(shaped)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `result.hits` — the facet envelope, shaped as canonical `[{value, count}]`.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_facets(result: &Value) -> Vec<Value> {
    result
        .get("result")
        .and_then(|result| result.get("hits"))
        .and_then(Value::as_array)
        .map(|hits| {
            hits.iter()
                .map(|hit| {
                    serde_json::json!({
                        "value": hit.get("value").cloned().unwrap_or(Value::Null),
                        "count": hit.get("count").cloned().unwrap_or(Value::from(0)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `result.count` — the count envelope.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_count(result: &Value) -> u64 {
    result
        .get("result")
        .and_then(|result| result.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// `result.collections[*].name` — canonical collection-name strings.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_collection_names(result: &Value) -> Vec<Value> {
    result
        .get("result")
        .and_then(|result| result.get("collections"))
        .and_then(Value::as_array)
        .map(|collections| {
            collections
                .iter()
                .filter_map(|entry| entry.get("name").cloned())
                .collect()
        })
        .unwrap_or_default()
}

/// `result.shard_keys[*].key` — canonical `PlanShardKey` values (string or
/// unsigned number).
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_shard_keys(result: &Value) -> Vec<Value> {
    match result
        .get("result")
        .and_then(|result| result.get("shard_keys"))
    {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|entry| entry.get("key").cloned())
            .collect(),
        _ => Vec::new(),
    }
}

/// `result.config` — canonical `QuotaConfig` object.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_quotas(result: &Value) -> Value {
    result
        .get("result")
        .and_then(|result| result.get("config"))
        .and_then(|config| serde_json::from_value::<qql_plan::QuotaConfig>(config.clone()).ok())
        .and_then(|config| serde_json::to_value(config).ok())
        .unwrap_or(Value::Null)
}

/// `result` — canonical `CollectionInfo` object. Mirrors the runtime's
/// `backend::schema_from_rest_result` derivation so browser callers read the
/// same shape as the native SDKs.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn result_collection_info(result: &Value) -> Value {
    let config = result.get("config");
    let params = config.and_then(|c| c.get("params"));

    let mut dense_vectors: Vec<String> = Vec::new();
    let mut vectors: Vec<Value> = Vec::new();
    if let Some(vectors_map) = params
        .and_then(|p| p.get("vectors"))
        .and_then(Value::as_object)
    {
        if vectors_map.contains_key("size") && vectors_map.contains_key("distance") {
            // Unnamed default dense vector.
            if let Some(spec) = vector_spec(None, &Value::Object(vectors_map.clone())) {
                vectors.push(spec);
            }
        } else {
            for (name, cfg) in vectors_map {
                if is_pseudo_vector_key(name) {
                    continue;
                }
                dense_vectors.push(name.clone());
                if let Some(spec) = vector_spec(Some(name.clone()), cfg) {
                    vectors.push(spec);
                }
            }
            dense_vectors.sort();
            // Runtime sorts full specs by name; unnamed specs (only possible
            // in the single-vector branch) keep insertion order.
            vectors.sort_by(|a, b| {
                a.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .cmp(b.get("name").and_then(Value::as_str).unwrap_or_default())
            });
        }
    }

    let sparse_vectors: Vec<Value> = params
        .and_then(|p| p.get("sparse_vectors"))
        .and_then(Value::as_object)
        .map(|sparse| {
            let mut specs: Vec<(String, Value)> = sparse
                .iter()
                .map(|(name, cfg)| {
                    let mut spec = Map::new();
                    spec.insert("name".into(), Value::from(name.clone()));
                    if let Some(index) = cfg.get("index").filter(|i| i.is_object()) {
                        spec.insert("index".into(), index.clone());
                    }
                    if let Some(modifier) = cfg.get("modifier").and_then(Value::as_str) {
                        spec.insert("modifier".into(), Value::from(modifier));
                    }
                    (name.clone(), Value::Object(spec))
                })
                .collect();
            specs.sort_by(|a, b| a.0.cmp(&b.0));
            specs.into_iter().map(|(_, spec)| spec).collect()
        })
        .unwrap_or_default();

    let mut payload_indexes: Vec<Value> = result
        .get("payload_schema")
        .and_then(Value::as_object)
        .map(|schema| {
            schema
                .iter()
                .map(|(field, meta)| {
                    let data_type = meta
                        .get("data_type")
                        .and_then(Value::as_str)
                        .or_else(|| meta.get("type").and_then(Value::as_str))
                        .unwrap_or("keyword")
                        .to_ascii_lowercase();
                    let mut index_params = Map::new();
                    if let Some(params) = meta.get("params").and_then(Value::as_object) {
                        for (key, value) in params {
                            if key != "type" {
                                index_params.insert(key.clone(), value.clone());
                            }
                        }
                    }
                    let is_tenant = meta
                        .get("is_tenant")
                        .and_then(Value::as_bool)
                        .or_else(|| index_params.get("is_tenant").and_then(Value::as_bool));
                    let mut spec = Map::new();
                    spec.insert("field".into(), Value::from(field.clone()));
                    spec.insert("data_type".into(), Value::from(data_type));
                    spec.insert("params".into(), Value::Object(index_params));
                    if let Some(is_tenant) = is_tenant {
                        spec.insert("is_tenant".into(), Value::from(is_tenant));
                    }
                    Value::Object(spec)
                })
                .collect()
        })
        .unwrap_or_default();
    payload_indexes.sort_by(|a, b| {
        a.get("field")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(b.get("field").and_then(Value::as_str).unwrap_or_default())
    });

    let mut collection_params = Map::new();
    if let Some(params) = params {
        for key in [
            "shard_number",
            "sharding_method",
            "on_disk_payload",
            "replication_factor",
        ] {
            if let Some(value) = params.get(key)
                && !value.is_null()
            {
                collection_params.insert(key.into(), value.clone());
            }
        }
        if let Some(memory) = params
            .get("payload")
            .and_then(|payload| payload.get("memory"))
            .and_then(Value::as_str)
        {
            collection_params.insert("payload_memory".into(), Value::from(memory));
        }
    }

    let mut schema = Map::new();
    schema.insert("dense_vectors".into(), Value::from(dense_vectors));
    schema.insert("sparse_vectors".into(), Value::Array(sparse_vectors));
    schema.insert("vectors".into(), Value::Array(vectors));
    schema.insert("payload_indexes".into(), Value::Array(payload_indexes));
    schema.insert("params".into(), Value::Object(collection_params));
    if let Some(hnsw) = config
        .and_then(|c| c.get("hnsw_config"))
        .and_then(Value::as_object)
    {
        schema.insert("hnsw".into(), Value::Object(filter_keys(hnsw, HNSW_KEYS)));
    }
    if let Some(optimizers) = config
        .and_then(|c| {
            c.get("optimizer_config")
                .or_else(|| c.get("optimizers_config"))
        })
        .and_then(Value::as_object)
    {
        schema.insert(
            "optimizers".into(),
            Value::Object(filter_keys(optimizers, OPTIMIZER_KEYS)),
        );
    }
    if let Some(quantization) = config
        .and_then(|c| c.get("quantization_config"))
        .filter(|q| !q.is_null())
    {
        schema.insert("quantization".into(), quantization.clone());
    }

    serde_json::json!({
        "status": result.get("status").and_then(Value::as_str).unwrap_or_default(),
        "points_count": result.get("points_count").and_then(Value::as_u64).unwrap_or(0),
        "segments_count": result.get("segments_count").and_then(Value::as_u64).unwrap_or(0),
        "schema": Value::Object(schema),
    })
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
const HNSW_KEYS: &[&str] = &[
    "m",
    "ef_construct",
    "full_scan_threshold",
    "max_indexing_threads",
    "on_disk",
    "payload_m",
    "inline_storage",
    "memory",
];

#[cfg(all(feature = "client", target_arch = "wasm32"))]
const OPTIMIZER_KEYS: &[&str] = &[
    "deleted_threshold",
    "vacuum_min_vector_number",
    "default_segment_number",
    "max_segment_size",
    "memmap_threshold",
    "indexing_threshold",
    "flush_interval_sec",
    "max_optimization_threads",
    "prevent_unoptimized",
];

/// Keep only the keys the runtime's DDL re-parsing understands.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn filter_keys(map: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for key in keys {
        if let Some(value) = map.get(*key)
            && !value.is_null()
        {
            out.insert((*key).to_string(), value.clone());
        }
    }
    out
}

/// Pseudo-keys of the unnamed-vector `vectors` object (never vector names).
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn is_pseudo_vector_key(name: &str) -> bool {
    matches!(
        name,
        "size"
            | "distance"
            | "hnsw_config"
            | "quantization_config"
            | "multivector_config"
            | "on_disk"
            | "datatype"
    )
}

/// One canonical `VectorSpec` object (`name` is `null` for the default vector).
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn vector_spec(name: Option<String>, cfg: &Value) -> Option<Value> {
    let size = cfg.get("size").and_then(Value::as_u64)?;
    let mut spec = Map::new();
    spec.insert("name".into(), name.map(Value::from).unwrap_or(Value::Null));
    spec.insert("size".into(), Value::from(size));
    spec.insert(
        "distance".into(),
        Value::from(
            cfg.get("distance")
                .and_then(Value::as_str)
                .unwrap_or("Cosine"),
        ),
    );
    if let Some(hnsw) = cfg.get("hnsw_config").filter(|h| h.is_object()) {
        spec.insert("hnsw".into(), hnsw.clone());
    }
    if let Some(quantization) = cfg.get("quantization_config").filter(|q| !q.is_null()) {
        spec.insert("quantization".into(), quantization.clone());
    }
    if let Some(multivector) = cfg.get("multivector_config").filter(|m| m.is_object()) {
        spec.insert("multivector".into(), multivector.clone());
    }
    if let Some(on_disk) = cfg.get("on_disk").and_then(Value::as_bool) {
        spec.insert("on_disk".into(), Value::from(on_disk));
    }
    if let Some(datatype) = cfg.get("datatype").and_then(Value::as_str) {
        spec.insert("datatype".into(), Value::from(datatype));
    }
    if let Some(memory) = cfg.get("memory").and_then(Value::as_str) {
        spec.insert("memory".into(), Value::from(memory));
    }
    Some(Value::Object(spec))
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn wasm_success_response(
    operation: &qql_plan::PlannedOperation,
    result: serde_json::Value,
) -> serde_json::Value {
    use qql_plan::PlannedOperation;

    let telemetry = telemetry_from_envelope(&result);
    let label = operation.operation_label();
    let (message, data) = match operation {
        PlannedOperation::Query { .. } | PlannedOperation::Scroll { .. } => {
            let hits = hit_array(result_points(&result));
            let count = hits.len();
            (format!("Found {count} hits"), Some(Value::Array(hits)))
        }
        PlannedOperation::GetPoints { .. } => {
            let hits = hit_array(result_records(&result));
            let count = hits.len();
            (format!("Found {count} hits"), Some(Value::Array(hits)))
        }
        PlannedOperation::QueryGroups { request, .. } => {
            let mut groups = result_groups(&result);
            // `group_offset` has no wire representation; trim client-side,
            // exactly like the runtime's normalization.
            if let Some(offset) = request.group_offset {
                let offset = offset as usize;
                if offset < groups.len() {
                    groups.drain(0..offset);
                } else {
                    groups.clear();
                }
            }
            let count = groups.len();
            (
                format!("Found {count} group(s)"),
                Some(serde_json::json!({"groups": groups})),
            )
        }
        PlannedOperation::Count { .. } => {
            let count = result_count(&result);
            (
                format!("Count: {count}"),
                Some(serde_json::json!({"count": count})),
            )
        }
        PlannedOperation::Facet { .. } => {
            let hits = result_facets(&result);
            let count = hits.len();
            (
                format!("Found {count} facet hit(s)"),
                Some(Value::Array(hits)),
            )
        }
        PlannedOperation::Upsert { request, .. } => (
            format!("Upserted {} point(s)", request.points.len()),
            Some(serde_json::json!({"count": request.points.len()})),
        ),
        PlannedOperation::ListCollections => {
            let names = result_collection_names(&result);
            let count = names.len();
            (
                format!("Found {count} collection(s)"),
                Some(serde_json::json!({"collections": names})),
            )
        }
        PlannedOperation::GetCollection { .. } => {
            (format!("{label} ok"), Some(result_collection_info(&result)))
        }
        PlannedOperation::ListShardKeys { .. } => (
            "Shard keys listed".to_string(),
            Some(serde_json::json!({"shard_keys": result_shard_keys(&result)})),
        ),
        PlannedOperation::GetQuotas => (
            "Quota configuration shown".to_string(),
            Some(result_quotas(&result)),
        ),
        PlannedOperation::SetQuotas { request } => (
            "Quota configuration updated".to_string(),
            Some(serde_json::to_value(&request.config).unwrap_or(serde_json::Value::Null)),
        ),
        _ => (format!("{label} ok"), None),
    };
    exec_response_with_telemetry(true, label, &message, data, telemetry)
}
