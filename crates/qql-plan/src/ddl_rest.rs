//! OpenAPI REST body projection for collection / index DDL.
//!
//! Each function returns a typed view over the plan IR; the caller serializes
//! it (`routing::serialize_body` / the runtime REST adapter). No JSON maps are
//! built here.

use crate::types::*;
use alloc::collections::BTreeMap;
use serde::Serialize;

// ── REST OpenAPI wire projection (distinct from internal plan IR) ─────────
//
// CreateCollection OpenAPI fields are top-level (replication_factor, …), not a
// nested `params` object. QuantizationConfig is already the nested OpenAPI
// shape in the typed IR. Plan IR may carry `shard_keys`; the REST projection
// creates them via the /shards endpoint after collection create (not as a
// CreateCollection field).

/// OpenAPI PUT `/collections/{c}` body.
#[derive(Debug, Serialize)]
pub struct CreateCollectionRestBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vectors: Option<&'a DenseVectorsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sparse_vectors: Option<&'a BTreeMap<String, SparseVectorParams>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hnsw_config: Option<&'a HnswConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optimizers_config: Option<&'a OptimizersConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quantization_config: Option<&'a QuantizationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shard_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sharding_method: Option<ShardingMethod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replication_factor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    write_consistency_factor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    on_disk_payload: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a PayloadStorageParams>,
}

/// Build the OpenAPI PUT `/collections/{c}` body view from plan IR.
pub fn create_collection_rest_body(req: &CreateCollectionRequest) -> CreateCollectionRestBody<'_> {
    let params = req.params.as_ref();
    CreateCollectionRestBody {
        vectors: req.vectors.as_ref(),
        sparse_vectors: req.sparse_vectors.as_ref(),
        hnsw_config: req.hnsw_config.as_ref(),
        optimizers_config: req.optimizers_config.as_ref(),
        quantization_config: req.quantization_config.as_ref(),
        shard_number: req.shard_number,
        sharding_method: req.sharding_method,
        // OpenAPI CreateCollection: replication_factor / write_consistency_factor /
        // on_disk_payload / payload are top-level, not nested under `params`.
        replication_factor: params.and_then(|p| p.replication_factor),
        write_consistency_factor: params.and_then(|p| p.write_consistency_factor),
        on_disk_payload: params.and_then(|p| p.on_disk_payload),
        payload: params.and_then(|p| p.payload.as_ref()),
        // read_fan_out_* only exist on UpdateCollection params (CollectionParamsDiff);
        // callers apply them with a follow-up PATCH (REST) or update (gRPC).
    }
}

/// OpenAPI PUT `/collections/{c}/index` body.
///
/// When index options are present, `field_schema` becomes a typed object
/// (`{ "type": "text", "tokenizer": … }`) per OpenAPI `PayloadSchemaParams`.
/// Without options it remains a plain type string.
#[derive(Debug, Serialize)]
pub struct CreateIndexRestBody<'a> {
    field_name: &'a str,
    field_schema: FieldSchema<'a>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum FieldSchema<'a> {
    /// `"field_schema": "text"`
    Type(IndexFieldType),
    /// `"field_schema": { "type": "text", …options }`
    Params {
        #[serde(rename = "type")]
        field_type: IndexFieldType,
        #[serde(flatten)]
        options: &'a IndexOptions,
    },
}

/// Build the OpenAPI PUT `/collections/{c}/index` body view from plan IR.
pub fn create_index_rest_body(req: &CreateIndexRequest) -> CreateIndexRestBody<'_> {
    let field_schema = if req.options.is_empty() {
        FieldSchema::Type(req.field_schema)
    } else {
        FieldSchema::Params {
            field_type: req.field_schema,
            options: &req.options,
        }
    };
    CreateIndexRestBody {
        field_name: &req.field_name,
        field_schema,
    }
}

/// Follow-up PATCH body for create-time params that only exist on update
/// (`read_fan_out_factor`, `read_fan_out_delay_ms`).
#[derive(Debug, Serialize)]
pub struct CreateCollectionDeferredParams {
    params: DeferredCollectionParams,
}

#[derive(Debug, Serialize)]
struct DeferredCollectionParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    read_fan_out_factor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_fan_out_delay_ms: Option<u64>,
}

/// Build the deferred PATCH body view, or `None` when no deferred param is set.
pub fn create_collection_deferred_params_rest(
    req: &CreateCollectionRequest,
) -> Option<CreateCollectionDeferredParams> {
    let params = req.params.as_ref()?;
    if params.read_fan_out_factor.is_none() && params.read_fan_out_delay_ms.is_none() {
        return None;
    }
    Some(CreateCollectionDeferredParams {
        params: DeferredCollectionParams {
            read_fan_out_factor: params.read_fan_out_factor,
            read_fan_out_delay_ms: params.read_fan_out_delay_ms,
        },
    })
}
