//! Typed AST for DDL statements and collection configuration.

use super::types::*;
use crate::ast::Value;
use alloc::string::String;
use alloc::vec::Vec;

/// `WITH MULTIVECTOR (…)` settings on a vector definition.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultivectorConfig {
    /// Scoring comparator (currently `max_sim`).
    pub comparator: MultivectorComparator,
}

/// One named dense vector definition of `CREATE COLLECTION`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorDef {
    /// Vector name.
    pub name: String,
    /// Vector dimension.
    pub size: u64,
    /// Distance metric.
    pub distance: VectorDistance,
    /// Per-vector `WITH HNSW` overrides.
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    /// Per-vector `WITH QUANTIZATION` settings.
    pub quantization: Option<Box<QuantizationConfig>>,
    /// `WITH MULTIVECTOR` settings for late-interaction vectors.
    pub multivector: Option<MultivectorConfig>,
    /// `WITH VECTOR` storage settings (memory placement, datatype).
    pub vectors: Option<Box<VectorsConfig>>,
}

/// `WITH SPARSE (…)` index settings for a sparse vector definition.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SparseIndexConfig {
    /// Vector count below which full scans are used.
    pub full_scan_threshold: Option<u64>,
    /// Legacy flag storing the index on disk (prefer `memory`).
    pub on_disk: Option<bool>,
    /// Storage datatype (`float32` / `float16` / `uint8`). Turbo4 is rejected.
    pub datatype: Option<VectorDatatype>,
    /// Memory placement of the index.
    pub memory: Option<MemoryPlacement>,
}

/// One named sparse vector definition of `CREATE COLLECTION`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SparseVectorDef {
    /// Sparse vector name.
    pub name: String,
    /// Optional `WITH SPARSE` index settings.
    pub index: Option<Box<SparseIndexConfig>>,
    /// Optional index modifier, e.g. `idf`.
    pub modifier: Option<String>,
}

/// `WITH QUANTIZATION (…)` settings for a vector definition.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationConfig {
    /// Quantization family.
    pub qtype: QuantizationType,
    /// Legacy flag keeping quantized vectors in RAM (prefer `memory`).
    pub always_ram: bool,
    /// Scalar quantization quantile in `[0, 1]`.
    pub quantile: Option<f64>,
    /// Bits per dimension (Turbo accepts 1, 1.5, 2, or 4).
    pub bits: Option<f64>,
    /// Product quantization compression (`x4`–`x64`).
    pub compression: Option<String>,
    /// Binary quantization encoding (`one_bit`, `two_bits`, `one_and_half_bits`).
    pub encoding: Option<String>,
    /// Binary query encoding (`default`, `binary`, `scalar4bits`, `scalar8bits`).
    pub query_encoding: Option<String>,
    /// Memory placement of quantized vectors.
    pub memory: Option<MemoryPlacement>,
}

/// `WITH QUANTIZATION` replacement emitted by `ALTER COLLECTION`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationUpdate {
    /// `disabled = true` — drop the collection's quantization config.
    pub disabled: bool,
    /// Replacement quantization settings; `None` only when disabled.
    pub config: Option<Box<QuantizationConfig>>,
}

/// `WITH HNSW (…)` graph settings (collection- or vector-scoped).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HnswRuntimeConfig {
    /// Number of edges per node in the HNSW graph.
    pub m: Option<u64>,
    /// Size of the candidate list used while building the graph.
    pub ef_construct: Option<u64>,
    /// Vector count below which full scans are used.
    pub full_scan_threshold: Option<u64>,
    /// Parallel indexing thread cap (`0` = automatic).
    pub max_indexing_threads: Option<u64>,
    /// Legacy flag storing the graph on disk (prefer `memory`).
    pub on_disk: Option<bool>,
    /// Extra graph edges for payload-aware links.
    pub payload_m: Option<u64>,
    /// Inline-storage flag for the HNSW graph.
    pub inline_storage: Option<bool>,
    /// Memory placement of the HNSW graph.
    pub memory: Option<MemoryPlacement>,
}

/// `WITH VECTOR (…)` storage settings.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorsConfig {
    /// Legacy flag storing original vectors on disk (prefer `memory`).
    pub on_disk: Option<bool>,
    /// Memory placement of the original vector storage.
    pub memory: Option<MemoryPlacement>,
    /// Storage datatype for dense vectors.
    pub datatype: Option<VectorDatatype>,
}

/// `max_optimization_threads` value with `auto` support.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OptimizationThreads {
    /// `auto` — let the server choose the thread count.
    pub auto_: bool,
    /// Explicit thread count used when `auto_` is false.
    pub value: u64,
}

/// `WITH OPTIMIZERS (…)` background segment and optimizer settings.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OptimizersRuntimeConfig {
    /// Deleted-record ratio (`0.0..=1.0`) that triggers a segment rebuild.
    pub deleted_threshold: Option<f64>,
    /// Minimum vector count before a segment is optimized.
    pub vacuum_min_vector_number: Option<u64>,
    /// Target number of segments.
    pub default_segment_number: Option<u64>,
    /// Maximum segment size, in kilobytes.
    pub max_segment_size: Option<u64>,
    /// Vector count above which a segment switches to memmap storage.
    pub memmap_threshold: Option<u64>,
    /// Vector count below which a segment is not HNSW-indexed.
    pub indexing_threshold: Option<u64>,
    /// Seconds between automatic storage flushes.
    pub flush_interval_sec: Option<u64>,
    /// Optimization thread budget (explicit or `auto`).
    pub max_optimization_threads: Option<OptimizationThreads>,
    /// Suspend background optimization while set.
    pub prevent_unoptimized: Option<bool>,
}

/// `WITH PARAMS (…)` collection-level cluster and storage parameters.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CollectionParamsConfig {
    /// Number of replicas per shard.
    pub replication_factor: Option<u64>,
    /// Minimum replicas that must acknowledge a write.
    pub write_consistency_factor: Option<u64>,
    /// Number of replicas queried in parallel for reads.
    pub read_fan_out_factor: Option<u64>,
    /// Delay between read fan-out attempts, in milliseconds.
    pub read_fan_out_delay_ms: Option<u64>,
    /// Legacy flag storing payload on disk (prefer `payload_memory`).
    pub on_disk_payload: Option<bool>,
    /// Memory placement of the payload storage (`Cold` / `Cached` only; never `Pinned`).
    pub payload_memory: Option<MemoryPlacement>,
    /// Total number of shards.
    pub shard_number: Option<u64>,
    /// Shard placement method: `auto` or `custom`.
    pub sharding_method: Option<String>,
    /// Tenant keys enabled for custom sharding.
    pub shard_keys: Option<Vec<String>>,
}

/// Full `WITH`-clause configuration of a collection.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CollectionConfig {
    /// Default dense vector storage settings.
    pub vectors: Option<Box<VectorsConfig>>,
    /// Collection-wide HNSW settings.
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    /// Collection-wide optimizer settings.
    pub optimizers: Option<Box<OptimizersRuntimeConfig>>,
    /// Cluster and storage parameters.
    pub params: Option<Box<CollectionParamsConfig>>,
    /// Collection-wide quantization settings.
    pub quantization: Option<Box<QuantizationConfig>>,
    /// Quantization replacement emitted by `ALTER COLLECTION`.
    pub quantization_update: Option<Box<QuantizationUpdate>>,
}

/// `CREATE COLLECTION` topology mode.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CollectionMode {
    /// `USING [DENSE] MODEL '…'` — single dense vector sized by the model.
    Dense {
        /// Model whose embedding dimension defines the vector.
        model: Option<String>,
    },
    /// `HYBRID` / `USING HYBRID` — dense + sparse topology.
    Hybrid {
        /// Name assigned to the dense role vector.
        dense_vector: Option<String>,
        /// Name assigned to the sparse role vector.
        sparse_vector: Option<String>,
    },
    /// `HYBRID RERANK` — dense + sparse + `colbert` multivector topology.
    Rerank,
}

/// `CREATE COLLECTION <name>` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreateCollectionStmt {
    /// Name of the collection to create.
    pub collection: String,
    /// Topology mode (model, hybrid, or rerank).
    pub mode: CollectionMode,
    /// Named dense vector definitions.
    pub vectors: Vec<VectorDef>,
    /// Named sparse vector definitions.
    pub sparse_vectors: Vec<SparseVectorDef>,
    /// `WITH` config blocks (HNSW, PARAMS, OPTIMIZERS, QUANTIZATION, VECTOR).
    pub config: Option<Box<CollectionConfig>>,
}

/// `ALTER COLLECTION <name>` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlterCollectionStmt {
    /// Collection to alter.
    pub collection: String,
    /// Replacement `WITH` config blocks.
    pub config: Option<Box<CollectionConfig>>,
}

/// `DROP COLLECTION <name>` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DropCollectionStmt {
    /// Collection to drop.
    pub collection: String,
}

/// `CREATE INDEX ON COLLECTION <c> FOR <field> [TYPE t] [WITH (…)]`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreateIndexStmt {
    /// Collection to index.
    pub collection: String,
    /// Payload field to index.
    pub field: String,
    /// Index type (keyword, integer, float, geo, text, bool, datetime, uuid).
    pub field_type: String,
    /// `WITH (…)` index options (e.g. `is_tenant`, `prefix`).
    pub options: Vec<(String, Value)>,
    /// Optional index creation durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// `DROP INDEX ON COLLECTION <c> FOR <field>` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DropIndexStmt {
    /// Collection to update.
    pub collection: String,
    /// Indexed field to drop.
    pub field: String,
}

/// `CREATE SHARD KEY '<key>' ON COLLECTION <c> [WITH (…)]`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreateShardKeyStmt {
    /// Collection to partition.
    pub collection: String,
    /// Shard key value to register.
    pub shard_key: String,
    /// Number of shards behind this key.
    pub shards_number: Option<u64>,
    /// Replication factor for these shards.
    pub replication_factor: Option<u64>,
}

/// `DROP SHARD KEY '<key>' ON COLLECTION <c>`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DropShardKeyStmt {
    /// Collection to update.
    pub collection: String,
    /// Shard key value to remove.
    pub shard_key: String,
}

/// Global quota configuration statement (`SET QUOTA (…) [WAIT bool]`).
///
/// `config` keeps the raw `key = value` pairs (enabled,
/// max_resident_memory_percent, max_disk_usage_percent,
/// release_margin_percent); the plan layer validates and serializes them.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SetQuotaStmt {
    /// Raw `key = value` quota settings; validated and serialized by the planner.
    pub config: Vec<(String, Value)>,
    /// `WAIT` — block until the new limits take effect.
    pub wait: Option<bool>,
}
