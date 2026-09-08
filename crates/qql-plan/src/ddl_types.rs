//! Collection / index / shard-key / quota IR request types.

use alloc::string::String;
use alloc::vec::Vec;
use qql_core::ast::MemoryPlacement;
use serde::Serialize;

/// HNSW index configuration for collection creation/update.
#[derive(Debug, Clone, Serialize)]
pub struct HnswConfig {
    /// Edges per node (`m`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<u64>,
    /// Candidate list size while building (`ef_construct`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ef_construct: Option<u64>,
    /// Brute-force fallback below this point count (`full_scan_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_scan_threshold: Option<u64>,
    /// Indexing thread cap (`max_indexing_threads`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_indexing_threads: Option<u64>,
    /// Store the HNSW graph on disk (`on_disk`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Edges per node for payload-aware indexes (`payload_m`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_m: Option<u64>,
    /// Keep the graph inline with vector storage (`inline_storage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_storage: Option<bool>,
    /// Memory placement of the HNSW graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// Segment optimizer configuration for collection creation/update.
#[derive(Debug, Clone, Serialize)]
pub struct OptimizersConfig {
    /// Deleted-vector ratio that triggers segment merges (`deleted_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_threshold: Option<f64>,
    /// Minimum segment size for vacuuming (`vacuum_min_vector_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vacuum_min_vector_number: Option<u64>,
    /// Target segment count (`default_segment_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_segment_number: Option<u64>,
    /// Maximum segment size in bytes (`max_segment_size`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_segment_size: Option<u64>,
    /// Point count above which segments are memmaped (`memmap_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memmap_threshold: Option<u64>,
    /// Minimum points before indexing kicks in (`indexing_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing_threshold: Option<u64>,
    /// Background flush interval in seconds (`flush_interval_sec`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flush_interval_sec: Option<u64>,
    /// Either a `u64` number or the string `"auto"` (REST-only; gRPC ignores "auto").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_optimization_threads: Option<serde_json::Value>,
    /// Reject queries over unoptimized segments (`prevent_unoptimized`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prevent_unoptimized: Option<bool>,
}

/// Vector quantization config (scalar/product/binary/turbo).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum QuantizationConfig {
    /// Scalar (int8) quantization (`{"scalar": …}`).
    Scalar {
        /// Scalar quantization parameters.
        scalar: ScalarQuantization,
    },
    /// Product quantization (`{"product": …}`).
    Product {
        /// Product quantization parameters.
        product: ProductQuantization,
    },
    /// Binary quantization (`{"binary": …}`).
    Binary {
        /// Binary quantization parameters.
        binary: BinaryQuantization,
    },
    /// OpenAPI `TurboQuantization`: `{ "turbo": { "bits": "bits2", … } }`.
    Turbo {
        /// Turbo quantization parameters.
        turbo: TurboQuantization,
    },
}

/// OpenAPI scalar quantization config (type `int8`).
#[derive(Debug, Clone, Serialize)]
pub struct ScalarQuantization {
    /// Qdrant REST/OpenAPI expects `"int8"` for scalar quantization type.
    #[serde(rename = "type")]
    pub qtype: String,
    /// Calibration quantile, e.g. `0.99`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantile: Option<f64>,
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI product quantization config.
#[derive(Debug, Clone, Serialize)]
pub struct ProductQuantization {
    /// Compression ratio: `x4`, `x8`, `x16`, or `x32`.
    pub compression: String,
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI binary quantization config.
#[derive(Debug, Clone, Serialize)]
pub struct BinaryQuantization {
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Bit packing: `one_bit`, `two_bits`, or `one_and_half_bits`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// Query-side encoding: `default`, `binary`, `scalar4bits`, `scalar8bits`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_encoding: Option<String>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI `TurboQuantQuantizationConfig`.
#[derive(Debug, Clone, Serialize)]
pub struct TurboQuantization {
    /// OpenAPI enum: `bits1` | `bits1_5` | `bits2` | `bits4`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<String>,
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// Plan IR for `CREATE COLLECTION`; projected to the OpenAPI body at the edge.
#[derive(Debug, Clone, Serialize)]
pub struct CreateCollectionRequest {
    /// Named dense vector configs (`size`, `distance`, per-vector options).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<serde_json::Map<String, serde_json::Value>>,
    /// Named sparse vector configs (`modifier`, optional `index` settings).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_vectors: Option<serde_json::Map<String, serde_json::Value>>,
    /// Collection-wide HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// Collection-wide optimizer settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    /// Collection params (`replication_factor`, `read_fan_out_*`, `payload`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Vector quantization settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfig>,
    /// Flat `vectors_config` (`on_disk`/`memory`/`datatype`) from `WITH VECTOR`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors_config: Option<serde_json::Value>,
    /// Number of shards (`shard_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_number: Option<u64>,
    /// `"auto"` or `"custom"` sharding method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharding_method: Option<String>,
    /// Custom shard keys created via `/shards` after collection create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_keys: Option<Vec<String>>,
    /// OpenAPI `PayloadStorageParams`: `{"memory": "cold"|"cached"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// Plan IR for `ALTER COLLECTION`; projected to the OpenAPI PATCH body.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCollectionRequest {
    /// Updated optimizer settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    /// Updated collection params.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Updated HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// PATCH envelope for update (`{disabled, quantization_config}`) — JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<serde_json::Value>,
}

/// Plan IR for `CREATE INDEX`; extra options stay flattened for gRPC.
#[derive(Debug, Clone, Serialize)]
pub struct CreateIndexRequest {
    /// Payload field to index.
    pub field_name: String,
    /// Schema type: `keyword`, `integer`, `float`, `text`, `bool`, …
    pub field_schema: String,
    /// Extra index options flattened onto the request (tokenizer, …).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Plan IR for creating a custom shard key on a collection.
#[derive(Debug, Clone, Serialize)]
pub struct CreateShardKeyRequest {
    /// Custom shard key to create.
    pub shard_key: String,
    /// Number of shards backing the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shards_number: Option<u64>,
    /// Replication factor for the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replication_factor: Option<u64>,
}

/// Plan IR for dropping a custom shard key from a collection.
#[derive(Debug, Clone, Serialize)]
pub struct DropShardKeyRequest {
    /// Custom shard key to remove.
    pub shard_key: String,
}

/// Cluster-wide resource quota configuration (`PUT /quotas`).
#[derive(Debug, Clone, Serialize)]
pub struct SetQuotaRequest {
    /// Whether quota enforcement is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Resident-memory cap as a percent of total (1-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_resident_memory_percent: Option<u64>,
    /// Disk-usage cap as a percent of total (1-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_disk_usage_percent: Option<u64>,
    /// Margin reclaimed when a cap trips, as a percent (0-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_margin_percent: Option<u64>,
    /// REST query param (`?wait=`), not body.
    #[serde(skip)]
    pub wait: Option<bool>,
}
