//! OpenAPI REST body projection for collection / index DDL.

use crate::quantization::{nest_quantization_for_rest, nest_vector_params_for_rest};
use crate::routing::{RestProjectionError, serialize_body};
use crate::types::*;

// ── REST OpenAPI wire projection (distinct from internal plan IR) ─────────
//
// CreateCollection OpenAPI fields are top-level (replication_factor, …), not a
// nested `params` object. QuantizationConfig is nested (`{ "scalar": {…} }`).
// Plan IR may carry `shard_keys`; the REST projection creates them via the
// /shards endpoint after collection create (not as a CreateCollection field).
// Internal plan IR keeps flat `type: "scalar"|…` for gRPC converters.

/// OpenAPI PUT `/collections/{c}` body from plan IR.
pub fn create_collection_rest_body(
    req: &CreateCollectionRequest,
) -> Result<serde_json::Value, RestProjectionError> {
    let mut body = serde_json::Map::new();

    if let Some(vectors) = &req.vectors {
        let mut out = serde_json::Map::new();
        for (name, cfg) in vectors {
            out.insert(name.clone(), nest_vector_params_for_rest(cfg));
        }
        body.insert("vectors".into(), serde_json::Value::Object(out));
    }
    if let Some(sparse) = &req.sparse_vectors {
        body.insert(
            "sparse_vectors".into(),
            serde_json::Value::Object(sparse.clone()),
        );
    }
    if let Some(hnsw) = &req.hnsw_config {
        let v = serialize_body(hnsw).map_err(|e| RestProjectionError::SerializeFailed {
            message: e.to_string(),
        })?;
        body.insert("hnsw_config".into(), v);
    }
    if let Some(opt) = &req.optimizers_config {
        let v = serialize_body(opt).map_err(|e| RestProjectionError::SerializeFailed {
            message: e.to_string(),
        })?;
        body.insert("optimizers_config".into(), v);
    }
    if let Some(q) = &req.quantization_config {
        let v = serialize_body(q).map_err(|e| RestProjectionError::SerializeFailed {
            message: e.to_string(),
        })?;
        body.insert("quantization_config".into(), v);
    }
    if let Some(n) = req.shard_number {
        body.insert("shard_number".into(), serde_json::Value::from(n));
    }
    if let Some(method) = &req.sharding_method {
        body.insert(
            "sharding_method".into(),
            serde_json::Value::String(method.clone()),
        );
    }
    // OpenAPI CreateCollection: replication_factor / write_consistency_factor /
    // on_disk_payload are top-level, not nested under `params`.
    if let Some(params) = &req.params {
        if let Some(rf) = params.get("replication_factor") {
            body.insert("replication_factor".into(), rf.clone());
        }
        if let Some(wc) = params.get("write_consistency_factor") {
            body.insert("write_consistency_factor".into(), wc.clone());
        }
        if let Some(od) = params.get("on_disk_payload") {
            body.insert("on_disk_payload".into(), od.clone());
        }
        // read_fan_out_* only exist on UpdateCollection params (CollectionParamsDiff);
        // callers apply them with a follow-up PATCH (REST) or update (gRPC).
    }
    if let Some(payload) = &req.payload {
        body.insert("payload".into(), payload.clone());
    }
    // Do not emit: params, vectors_config, shard_keys
    Ok(serde_json::Value::Object(body))
}

/// OpenAPI PUT `/collections/{c}/index` body.
///
/// When index options are present, `field_schema` becomes a typed object
/// (`{ "type": "text", "tokenizer": … }`) per OpenAPI `PayloadSchemaParams`.
/// Without options it remains a plain type string.
pub fn create_index_rest_body(req: &CreateIndexRequest) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert(
        "field_name".into(),
        serde_json::Value::String(req.field_name.clone()),
    );
    if req.extra.is_empty() {
        body.insert(
            "field_schema".into(),
            serde_json::Value::String(req.field_schema.clone()),
        );
    } else {
        let mut schema = serde_json::Map::new();
        schema.insert(
            "type".into(),
            serde_json::Value::String(req.field_schema.clone()),
        );
        for (k, v) in &req.extra {
            // Map QQL aliases to OpenAPI enum strings where needed.
            if k == "encoding" {
                continue;
            }
            let v = if k == "tokenizer" {
                if let Some(s) = v.as_str() {
                    serde_json::Value::String(s.to_ascii_lowercase())
                } else {
                    v.clone()
                }
            } else {
                v.clone()
            };
            schema.insert(k.clone(), v);
        }
        body.insert("field_schema".into(), serde_json::Value::Object(schema));
    }
    serde_json::Value::Object(body)
}

/// OpenAPI PATCH `/collections/{c}` body from plan IR.
pub fn update_collection_rest_body(
    req: &UpdateCollectionRequest,
) -> Result<serde_json::Value, RestProjectionError> {
    let mut body = serde_json::Map::new();
    if let Some(hnsw) = &req.hnsw_config {
        let v = serialize_body(hnsw).map_err(|e| RestProjectionError::SerializeFailed {
            message: e.to_string(),
        })?;
        body.insert("hnsw_config".into(), v);
    }
    if let Some(opt) = &req.optimizers_config {
        let v = serialize_body(opt).map_err(|e| RestProjectionError::SerializeFailed {
            message: e.to_string(),
        })?;
        body.insert("optimizers_config".into(), v);
    }
    if let Some(params) = &req.params {
        body.insert("params".into(), params.clone());
    }
    if let Some(q) = &req.quantization_config {
        body.insert("quantization_config".into(), nest_quantization_for_rest(q));
    }
    Ok(serde_json::Value::Object(body))
}

/// Follow-up PATCH body for create-time params that only exist on update
/// (`read_fan_out_factor`, `read_fan_out_delay_ms`).
pub fn create_collection_deferred_params_rest(
    req: &CreateCollectionRequest,
) -> Option<serde_json::Value> {
    let params = req.params.as_ref()?;
    let mut out = serde_json::Map::new();
    if let Some(v) = params.get("read_fan_out_factor") {
        out.insert("read_fan_out_factor".into(), v.clone());
    }
    if let Some(v) = params.get("read_fan_out_delay_ms") {
        out.insert("read_fan_out_delay_ms".into(), v.clone());
    }
    if out.is_empty() {
        None
    } else {
        Some(serde_json::json!({ "params": out }))
    }
}
