//! Collection metadata shaping for the WASM JS boundary.
//!
//! WASM cannot depend on `qql-runtime`, so `SHOW COLLECTION` responses are
//! re-shaped here into the exact JSON the runtime's `CollectionInfo`
//! serializes. The strictness boundary matches `rest_response.rs`: `result`
//! must be an object, `status` a string, and `segments_count` an unsigned
//! integer. Config/schema extraction is lenient exactly like the runtime's
//! `schema_from_rest_result` (a backend that omits optional config yields an
//! empty schema section rather than an error).

use serde_json::{Map, Value};

use qql_core::error::QqlError;

use super::response::envelope_err;

/// Parse `GET /collections/{name}` (`result` → canonical `CollectionInfo`).
pub(crate) fn parse_collection_info(envelope: &Value) -> Result<Value, QqlError> {
    let result = envelope
        .get("result")
        .filter(|value| value.is_object())
        .ok_or_else(|| envelope_err("get collection response is missing a result object"))?;
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| envelope_err("collection info is missing a string status"))?;
    let segments_count = result
        .get("segments_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| envelope_err("collection info is missing an unsigned segments_count"))?;
    // `points_count` is nullable in the OpenAPI schema; an unreported count
    // reads as zero, mirroring the runtime.
    let points_count = result
        .get("points_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut info = Map::new();
    info.insert("status".into(), Value::from(status));
    info.insert("points_count".into(), Value::from(points_count));
    if let Some(indexed_vectors_count) = result.get("indexed_vectors_count").and_then(Value::as_u64)
    {
        info.insert(
            "indexed_vectors_count".into(),
            Value::from(indexed_vectors_count),
        );
    }
    info.insert("segments_count".into(), Value::from(segments_count));
    info.insert("schema".into(), collection_schema_from_result(result));
    Ok(Value::Object(info))
}

/// Named vector topology from a collection `result` object — same extraction
/// the runtime's `schema_from_rest_result` performs for `USING` resolution.
pub(crate) fn vector_names_from_collection_result(result: &Value) -> qql_embed::TopologyNames {
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
                if is_pseudo_vector_key(name) {
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

/// `result` → canonical `CollectionSchema` JSON.
fn collection_schema_from_result(result: &Value) -> Value {
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
        if let Some(value) = params.get("shard_number").and_then(Value::as_u64) {
            collection_params.insert("shard_number".into(), Value::from(value));
        }
        if let Some(value) = params.get("sharding_method").and_then(Value::as_str) {
            collection_params.insert("sharding_method".into(), Value::from(value));
        }
        if let Some(value) = params.get("on_disk_payload").and_then(Value::as_bool) {
            collection_params.insert("on_disk_payload".into(), Value::from(value));
        }
        if let Some(value) = params
            .get("payload")
            .and_then(|payload| payload.get("memory"))
            .and_then(Value::as_str)
        {
            collection_params.insert("payload_memory".into(), Value::from(value));
        }
        if let Some(value) = params.get("replication_factor").and_then(Value::as_u64) {
            collection_params.insert("replication_factor".into(), Value::from(value));
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
    if let Some(quantization) = config.and_then(|c| c.get("quantization_config")) {
        schema.insert("quantization".into(), quantization.clone());
    }
    Value::Object(schema)
}

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
    if let Some(quantization) = cfg.get("quantization_config") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collection_info_requires_status_and_segments_count() {
        let err = parse_collection_info(&json!({"result": {}})).unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        let err = parse_collection_info(&json!({
            "result": {"status": "green"},
            "status": "ok",
        }))
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        let err = parse_collection_info(&json!({"result": [1], "status": "ok"})).unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn collection_info_matches_runtime_shape() {
        let info = parse_collection_info(&json!({
            "result": {
                "status": "green",
                "points_count": 12,
                "indexed_vectors_count": 9,
                "segments_count": 2,
                "config": {
                    "params": {
                        "vectors": {
                            "title": {"size": 4, "distance": "Dot"},
                            "body": {"size": 8, "distance": "Cosine", "multivector_config": {"comparator": "max_sim"}},
                        },
                        "sparse_vectors": {"text": {"modifier": "idf"}},
                        "shard_number": 3,
                        "sharding_method": "custom",
                        "on_disk_payload": true,
                        "replication_factor": 2,
                        "payload": {"memory": "cold"},
                    },
                    "hnsw_config": {"m": 16, "ef_construct": 100, "unknown": 1},
                    "optimizer_config": {"deleted_threshold": 0.2, "unknown": 2},
                    "quantization_config": {"scalar": {"type": "int8"}},
                },
                "payload_schema": {
                    "tenant_id": {"data_type": "keyword", "params": {"is_tenant": true}},
                    "title": {"data_type": "text"},
                },
            },
            "status": "ok",
        }))
        .unwrap();

        assert_eq!(info["status"], "green");
        assert_eq!(info["points_count"], 12);
        assert_eq!(info["indexed_vectors_count"], 9);
        assert_eq!(info["segments_count"], 2);
        let schema = &info["schema"];
        assert_eq!(schema["dense_vectors"], json!(["body", "title"]));
        assert_eq!(schema["vectors"][0]["name"], "body");
        assert_eq!(schema["vectors"][0]["size"], 8);
        assert_eq!(
            schema["vectors"][0]["multivector"],
            json!({"comparator": "max_sim"})
        );
        assert_eq!(schema["vectors"][1]["name"], "title");
        assert_eq!(schema["vectors"][1]["distance"], "Dot");
        assert_eq!(
            schema["sparse_vectors"],
            json!([{"name": "text", "modifier": "idf"}])
        );
        assert_eq!(
            schema["payload_indexes"],
            json!([
                {"field": "tenant_id", "data_type": "keyword", "params": {"is_tenant": true}, "is_tenant": true},
                {"field": "title", "data_type": "text", "params": {}},
            ])
        );
        assert_eq!(
            schema["params"],
            json!({
                "shard_number": 3,
                "sharding_method": "custom",
                "on_disk_payload": true,
                "payload_memory": "cold",
                "replication_factor": 2,
            })
        );
        assert_eq!(schema["hnsw"], json!({"m": 16, "ef_construct": 100}));
        assert_eq!(schema["optimizers"], json!({"deleted_threshold": 0.2}));
        assert_eq!(schema["quantization"], json!({"scalar": {"type": "int8"}}));
    }

    #[test]
    fn unnamed_vector_keeps_default_branch() {
        let info = parse_collection_info(&json!({
            "result": {
                "status": "yellow",
                "segments_count": 1,
                "config": {"params": {"vectors": {"size": 3, "distance": "Cosine"}}},
            },
        }))
        .unwrap();
        assert_eq!(info["points_count"], 0);
        assert!(info.get("indexed_vectors_count").is_none());
        assert_eq!(info["schema"]["dense_vectors"], json!([]));
        assert_eq!(
            info["schema"]["vectors"],
            json!([{"name": null, "size": 3, "distance": "Cosine"}])
        );
    }

    #[test]
    fn topology_names_from_collection_result() {
        let result = json!({
            "config": {"params": {
                "vectors": {
                    "dense_a": {"size": 4, "distance": "Dot"},
                    "colbert": {"size": 4, "distance": "Dot", "multivector_config": {"comparator": "max_sim"}},
                },
                "sparse_vectors": {"bm25": {}, "text": {}},
            }},
        });
        let names = vector_names_from_collection_result(&result);
        assert_eq!(names.dense, ["colbert", "dense_a"]);
        assert_eq!(names.multivector, ["colbert"]);
        assert_eq!(names.sparse, ["bm25", "text"]);
    }
}
