//! Collection / shard-key / quota IR request types.
//!
//! Mirrors the OpenAPI request schemas QQL can emit. Config types serialize to
//! the exact wire JSON; request structs serialize plan IR and are projected to
//! endpoint bodies by `ddl_rest` (create) or used directly (update).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use qql_core::ast::{MemoryPlacement, VectorDatatype, VectorDistance};
use serde::{Deserialize, Serialize, Serializer};

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

/// `max_optimization_threads` wire value: an explicit thread count or `"auto"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxOptimizationThreads {
    /// Let the server pick dynamically (`"auto"`).
    Auto,
    /// Explicit number of optimization threads.
    Threads(u64),
}

impl Serialize for MaxOptimizationThreads {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Threads(threads) => serializer.serialize_u64(*threads),
        }
    }
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
    /// Explicit thread count or `"auto"` (`max_optimization_threads`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_optimization_threads: Option<MaxOptimizationThreads>,
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

/// `UPDATE COLLECTION` quantization replacement: a config or `"Disabled"`.
#[derive(Debug, Clone)]
pub enum QuantizationConfigDiff {
    /// Drop the collection's quantization config (`"Disabled"`).
    Disabled,
    /// Replacement quantization settings.
    Config(QuantizationConfig),
}

impl Serialize for QuantizationConfigDiff {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Disabled => serializer.serialize_str("Disabled"),
            Self::Config(config) => config.serialize(serializer),
        }
    }
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
    /// Compression ratio: `x4`, `x8`, `x16`, `x32`, or `x64`.
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

/// Multivector (late-interaction) comparator; OpenAPI `max_sim` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiVectorComparator {
    /// Maximum pairwise vector similarity.
    MaxSim,
}

/// OpenAPI `MultiVectorConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MultiVectorConfig {
    /// Scoring comparator.
    pub comparator: MultiVectorComparator,
}

/// OpenAPI `VectorParams` (one dense vector's storage configuration).
#[derive(Debug, Clone, Serialize)]
pub struct DenseVectorParams {
    /// Vector dimension.
    pub size: u64,
    /// Distance metric; serializes as `Cosine` / `Dot` / `Euclid` / `Manhattan`.
    pub distance: VectorDistance,
    /// Per-vector HNSW overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// Per-vector quantization settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfig>,
    /// Legacy on-disk flag (prefer `memory`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Memory placement of the original vector storage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
    /// Storage datatype.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datatype: Option<VectorDatatype>,
    /// Multivector settings for late-interaction vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multivector_config: Option<MultiVectorConfig>,
}

/// OpenAPI `VectorsConfig`: a single unnamed config or a named map.
///
/// QQL lowering always emits [`DenseVectorsConfig::Named`] (vector definitions
/// are always named); `Single` mirrors the wire's unnamed form for total
/// conversion.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DenseVectorsConfig {
    /// Bare `VectorParams` object (single unnamed vector).
    Single(DenseVectorParams),
    /// `{ name: VectorParams }` map (named vectors).
    Named(BTreeMap<String, DenseVectorParams>),
}

/// Sparse index modifier (`none` | `idf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SparseModifier {
    /// No modification.
    None,
    /// Inverse document frequency.
    Idf,
}

/// OpenAPI `SparseIndexParams`: custom sparse index settings.
#[derive(Debug, Clone, Serialize)]
pub struct SparseIndexParams {
    /// Vector count below which full scans are used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_scan_threshold: Option<u64>,
    /// Legacy on-disk flag (prefer `memory`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Memory placement of the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
    /// Storage datatype (`float32` / `float16` / `uint8`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datatype: Option<VectorDatatype>,
}

/// OpenAPI `SparseVectorParams` (one sparse vector's configuration).
#[derive(Debug, Clone, Serialize)]
pub struct SparseVectorParams {
    /// Custom index settings; `None` uses the collection defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<SparseIndexParams>,
    /// Value modifier applied at query time.
    pub modifier: SparseModifier,
}

/// OpenAPI `VectorParamsDiff` (per-vector dense patch on `UpdateCollection`).
///
/// Field-wise: every unset key leaves the collection's current setting as-is.
/// The wire shape has no `datatype` field — a datatype change fails planning.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VectorParamsDiff {
    /// Replacement HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// Replacement quantization settings (`"Disabled"` clears them).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfigDiff>,
    /// Legacy on-disk flag (prefer `memory`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Memory placement of the original vector storage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI `SparseVectorParams` as a patch: `modifier` and every index key are
/// optional, so unset fields keep their current value.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SparseVectorParamsDiff {
    /// Replacement sparse index settings (field-wise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<SparseIndexParams>,
    /// Replacement value modifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifier: Option<SparseModifier>,
}

/// OpenAPI `PayloadStorageParams`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PayloadStorageParams {
    /// Memory placement of the payload storage (`cold`/`cached`; never `pinned`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI `CollectionParams` / `CollectionParamsDiff` fields QQL can set.
///
/// `CreateCollection` hoists replication/consistency/payload to the top level;
/// `UpdateCollection` nests the whole struct under `params`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CollectionParams {
    /// Number of replicas per shard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replication_factor: Option<u64>,
    /// Minimum replicas that must acknowledge a write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_consistency_factor: Option<u64>,
    /// Number of replicas queried in parallel for reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_fan_out_factor: Option<u64>,
    /// Delay between read fan-out attempts, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_fan_out_delay_ms: Option<u64>,
    /// Legacy flag storing payload on disk (prefer `payload.memory`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk_payload: Option<bool>,
    /// Payload storage placement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<PayloadStorageParams>,
}

/// Sharding method (`auto` | `custom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ShardingMethod {
    /// Points are distributed across all available shards.
    Auto,
    /// Points are distributed according to their shard key.
    Custom,
}

/// Plan IR for `CREATE COLLECTION`; projected to the OpenAPI body at the edge.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CreateCollectionRequest {
    /// Dense vector configs (single unnamed or named map).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<DenseVectorsConfig>,
    /// Named sparse vector configs (`modifier`, optional `index` settings).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_vectors: Option<BTreeMap<String, SparseVectorParams>>,
    /// Collection-wide HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// Collection-wide optimizer settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    /// Collection params (`replication_factor`, `read_fan_out_*`, payload).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<CollectionParams>,
    /// Vector quantization settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfig>,
    /// Number of shards (`shard_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_number: Option<u64>,
    /// `auto` or `custom` sharding method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharding_method: Option<ShardingMethod>,
    /// Custom shard keys created via `/shards` after collection create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_keys: Option<Vec<crate::semantic::PlanShardKey>>,
}

/// Plan IR for `ALTER COLLECTION`; serializes as the OpenAPI PATCH body.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateCollectionRequest {
    /// Updated optimizer settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    /// Updated collection params.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<CollectionParams>,
    /// Updated HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// Quantization replacement (`Disabled` or a config).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfigDiff>,
    /// Per-vector dense diffs: REST `VectorsConfigDiff` is this name-keyed map
    /// (`""` addresses the default unnamed vector).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<BTreeMap<String, VectorParamsDiff>>,
    /// Per-sparse-vector diffs (`sparse_vectors` on the PATCH body).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_vectors: Option<BTreeMap<String, SparseVectorParamsDiff>>,
}

/// Plan IR for creating a custom shard key on a collection.
#[derive(Debug, Clone, Serialize)]
pub struct CreateShardKeyRequest {
    /// Custom shard key to create (keyword or numeric).
    pub shard_key: crate::semantic::PlanShardKey,
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
    pub shard_key: crate::semantic::PlanShardKey,
}

/// Cluster-wide resource quota configuration (`GET`/`PUT /quotas`).
///
/// An unset field means the corresponding resource is uncapped. Used both as
/// the `PUT /quotas` request body and as the typed `GET /quotas` result
/// (`QuotaStatus.config`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// Whether quota enforcement is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Resident-memory cap as a percent of total (1-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_resident_memory_percent: Option<u64>,
    /// Disk-usage cap as a percent of total (1-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_disk_usage_percent: Option<u64>,
    /// Margin reclaimed when a cap trips, as a percent (0-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_margin_percent: Option<u64>,
}

/// Plan IR for `SET QUOTA`: the replacement [`QuotaConfig`] plus the REST-only
/// `?wait=` query flag.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SetQuotaRequest {
    /// Replacement configuration; omitted keys become uncapped defaults.
    #[serde(flatten)]
    pub config: QuotaConfig,
    /// REST query param (`?wait=`), not body.
    #[serde(skip)]
    pub wait: Option<bool>,
}
