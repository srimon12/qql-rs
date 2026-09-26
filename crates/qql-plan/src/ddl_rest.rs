//! OpenAPI REST body projection for collection / index DDL.
//!
//! Each function returns a typed view over the plan IR; the caller serializes
//! it (`routing::serialize_body` / the runtime REST adapter). No JSON maps are
//! built here.
//!
//! `create_collection_rest_steps` additionally sequences the multi-step REST
//! create (PUT, conditional PATCH, per-key shard PUTs) as pure data: the
//! runtime only sends the steps. Single-step DDL likewise builds through
//! `*_op` constructors so every transport shares one op-assembly point.

use crate::plan::PlannedOperation;
use crate::semantic::PlanShardKey;
use crate::types::*;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
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
    wal_config: Option<&'a WalConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    strict_mode_config: Option<&'a StrictModeConfig>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::value_serde::serialize_ast_pairs_opt_ref"
    )]
    metadata: Option<&'a Vec<(String, qql_core::ast::Value)>>,
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
        wal_config: req.wal_config.as_deref(),
        strict_mode_config: req.strict_mode_config.as_deref(),
        metadata: req.metadata.as_ref(),
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

/// One step of the multi-step `CREATE COLLECTION` REST sequence: HTTP verb,
/// absolute path, and serialized JSON body. Pure plan knowledge — the runtime
/// only sends the steps in order.
#[derive(Debug)]
pub struct RestDdlStep {
    /// HTTP verb for this step.
    pub method: Method,
    /// Absolute path with the collection interpolated.
    pub path: String,
    /// Serialized JSON body.
    pub body: serde_json::Value,
}

/// OpenAPI `PUT /collections/{collection}/shards` body for one custom shard key.
#[derive(Debug, Serialize)]
struct ShardKeyRestBody<'a> {
    shard_key: &'a PlanShardKey,
}

/// Expand a planned create into its REST sequence: `PUT` the collection body,
/// a conditional `PATCH` for update-only params, then one `PUT …/shards` per
/// custom shard key (the REST projection creates them after the collection,
/// never as a `CreateCollection` field).
pub fn create_collection_rest_steps(
    collection: &str,
    req: &CreateCollectionRequest,
) -> Result<Vec<RestDdlStep>, serde_json::Error> {
    let mut steps = Vec::new();
    steps.push(RestDdlStep {
        method: Method::Put,
        path: format!("/collections/{collection}"),
        body: serde_json::to_value(create_collection_rest_body(req))?,
    });
    if let Some(patch) = create_collection_deferred_params_rest(req) {
        steps.push(RestDdlStep {
            method: Method::Patch,
            path: format!("/collections/{collection}"),
            body: serde_json::to_value(patch)?,
        });
    }
    if let Some(keys) = &req.shard_keys {
        for key in keys {
            steps.push(RestDdlStep {
                method: Method::Put,
                path: format!("/collections/{collection}/shards"),
                body: serde_json::to_value(ShardKeyRestBody { shard_key: key })?,
            });
        }
    }
    Ok(steps)
}

/// Build the `PlannedOperation` for a trait-level update-collection call, so
/// every transport shares one op-assembly point (mirrors the gRPC DDL split).
pub fn update_collection_op(
    collection: &str,
    request: &UpdateCollectionRequest,
) -> PlannedOperation {
    PlannedOperation::UpdateCollection {
        collection: String::from(collection),
        request: request.clone(),
    }
}

/// Build the `PlannedOperation` for a trait-level create-index call.
/// Index creation always waits (matches the historical REST default).
pub fn create_index_op(collection: &str, request: &CreateIndexRequest) -> PlannedOperation {
    PlannedOperation::CreateIndex {
        collection: String::from(collection),
        request: request.clone(),
        wait: true,
    }
}

/// Build the `PlannedOperation` for a trait-level drop-index call.
pub fn drop_index_op(collection: &str, field: &str) -> PlannedOperation {
    PlannedOperation::DropIndex {
        collection: String::from(collection),
        field: String::from(field),
    }
}
