use super::{FilterExpr, FormulaExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Memory placement of a storage component (`cold` / `cached` / `pinned`).
///
/// Mirrors Qdrant 1.19 `Memory`. Data is always persisted on disk; this only
/// controls how the component is held in RAM. `Pinned` is not valid for
/// payload storage — parsers reject it for `payload_memory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum MemoryPlacement {
    Cold,
    Cached,
    Pinned,
}

impl MemoryPlacement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Cached => "cached",
            Self::Pinned => "pinned",
        }
    }

    /// Parse a placement string (case-insensitive). Unknown values → `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cold" => Some(Self::Cold),
            "cached" => Some(Self::Cached),
            "pinned" => Some(Self::Pinned),
            _ => None,
        }
    }
}

impl core::fmt::Display for MemoryPlacement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Dense / sparse vector storage datatype (OpenAPI `Datatype`).
///
/// Aliases accepted at parse: `f32`→`Float32`, `f16`→`Float16`, `u8`→`Uint8`,
/// `t4`→`Turbo4`. Sparse indexes reject `Turbo4` (unsupported upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum VectorDatatype {
    Float32,
    Float16,
    Uint8,
    Turbo4,
}

impl VectorDatatype {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Float32 => "float32",
            Self::Float16 => "float16",
            Self::Uint8 => "uint8",
            Self::Turbo4 => "turbo4",
        }
    }

    /// Parse a datatype string including short aliases. Unknown → `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "float32" | "f32" => Some(Self::Float32),
            "float16" | "f16" => Some(Self::Float16),
            "uint8" | "u8" => Some(Self::Uint8),
            "turbo4" | "t4" => Some(Self::Turbo4),
            _ => None,
        }
    }

    /// Sparse indexes support float32 / float16 / uint8 only (not turbo4).
    pub fn parse_sparse(s: &str) -> Option<Self> {
        match Self::parse(s) {
            Some(Self::Turbo4) => None,
            other => other,
        }
    }
}

impl core::fmt::Display for VectorDatatype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointId {
    Number(u64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorValue {
    Dense(Vec<f32>),
    Sparse { indices: Vec<u32>, values: Vec<f32> },
    MultiDense(Vec<Vec<f32>>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointVectors {
    Unnamed(VectorValue),
    Named(Vec<(String, VectorValue)>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryInput {
    Text {
        text: String,
        model: Option<String>,
    },
    /// Image path or URL for dense embedding (CLIP vision, etc.).
    /// Resolved to [`VectorValue::Dense`] before plan/dispatch.
    Image {
        source: String,
        model: Option<String>,
    },
    Vector(VectorValue),
    Point(PointId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorKind {
    Dense,
    Sparse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorTarget {
    pub name: String,
    pub kind: Option<VectorKind>,
    /// Multivector (ColBERT-style) dense target. Filled at parse only via
    /// `AS MULTI`, or at execution prep from collection schema
    /// (`multivector_config`). Not a third `VectorKind` — still dense.
    #[cfg_attr(feature = "serde", serde(default))]
    pub multi: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MmrConfig {
    pub diversity: f64,
    pub candidates: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContextPair {
    pub positive: QueryInput,
    pub negative: QueryInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RecommendStrategy {
    AverageVector,
    BestScore,
    SumScores,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FeedbackItem {
    pub example: QueryInput,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FeedbackStrategy {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FusionMethod {
    Rrf,
    Dbsf,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryCollection {
    Explicit(String),
    Inherited,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PrefetchSource {
    Cte(String),
    Query(Box<QueryStmt>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LookupSpec {
    pub collection: String,
    pub vector: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Prefetch {
    pub source: PrefetchSource,
    pub filter: Option<Box<FilterExpr>>,
    pub score_threshold: Option<f64>,
    pub lookup: Option<LookupSpec>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryExpr {
    Points {
        ids: Vec<PointId>,
    },
    Nearest {
        input: QueryInput,
        using: Option<VectorTarget>,
        prefetch: Vec<Prefetch>,
        mmr: Option<Box<MmrConfig>>,
    },
    Recommend {
        positive: Vec<QueryInput>,
        negative: Vec<QueryInput>,
        strategy: Option<RecommendStrategy>,
        using: Option<VectorTarget>,
        prefetch: Vec<Prefetch>,
    },
    Context {
        pairs: Vec<ContextPair>,
        using: Option<VectorTarget>,
        prefetch: Vec<Prefetch>,
    },
    Discover {
        target: QueryInput,
        context: Vec<ContextPair>,
        using: Option<VectorTarget>,
        prefetch: Vec<Prefetch>,
    },
    OrderBy {
        field: String,
        direction: OrderDirection,
    },
    SampleRandom,
    Fusion {
        method: FusionMethod,
        prefetch: Vec<Prefetch>,
    },
    Formula {
        expression: Box<FormulaExpr>,
        defaults: Vec<(String, Value)>,
        prefetch: Vec<Prefetch>,
    },
    RelevanceFeedback {
        target: QueryInput,
        feedback: Vec<FeedbackItem>,
        strategy: FeedbackStrategy,
        using: Option<VectorTarget>,
        prefetch: Vec<Prefetch>,
    },
    Hybrid {
        text: String,
        model: Option<String>,
        dense_vector: Option<String>,
        sparse_vector: Option<String>,
        fusion: FusionMethod,
    },
    Rerank {
        input: QueryInput,
        model: String,
        using: Option<VectorTarget>,
        prefetch: Vec<Prefetch>,
    },
    /// Cross-encoder pair rerank: score query against PREFETCH document texts.
    /// Not sent to Qdrant as MaxSim — executor scores client-side then reorders.
    CrossRerank {
        /// Query string scored against each document.
        query: String,
        /// Cross-encoder model id (e.g. bge-reranker-base).
        model: String,
        /// Payload field holding document text (default `"text"` at resolve time).
        field: Option<String>,
        prefetch: Vec<Prefetch>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationSearchParams {
    pub ignore: Option<bool>,
    pub rescore: Option<bool>,
    pub oversampling: Option<f64>,
}

/// Read consistency for Qdrant point reads.
///
/// OpenAPI `ReadConsistency` / proto `ReadConsistency`: either a replica
/// **factor** `N`, or a named mode (`majority` / `quorum` / `all`).
/// REST: query param on `/points/query` etc. gRPC: `read_consistency` field.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReadConsistency {
    /// Send requests to N nodes; keep points present on all of them.
    Factor(u64),
    /// N/2+1 random requests; points present on all of them.
    Majority,
    /// All nodes; points present on a majority.
    Quorum,
    /// All nodes; points present on all of them.
    All,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchParams {
    pub hnsw_ef: Option<u64>,
    pub exact: Option<bool>,
    pub acorn: Option<bool>,
    /// ACORN selectivity ceiling in (0, 1]. Only valid with `acorn = true`.
    pub max_selectivity: Option<f64>,
    pub indexed_only: Option<bool>,
    pub quantization: Option<QuantizationSearchParams>,
    pub rrf_k: Option<u64>,
    pub rrf_weights: Option<Vec<f64>>,
    /// Per-query IDF corpus for sparse vectors. `None` = collection-wide (global).
    pub idf: Option<IdfParams>,
    /// Request-level timeout in **seconds** (OpenAPI query param / proto field).
    /// Not part of body `SearchParams`.
    pub timeout: Option<u64>,
    /// Request-level read consistency (OpenAPI query param / proto field).
    pub consistency: Option<ReadConsistency>,
}

/// Sparse-vector IDF scope.
///
/// `corpus = None` is collection-wide (`PARAMS (idf = 'global')`). Otherwise
/// IDF statistics are computed over points matching the QQL filter
/// (`PARAMS (idf = WHERE tenant_id = 'acme')`). The planner lowers the filter
/// to a Qdrant `Filter`; the language never takes a JSON corpus object.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IdfParams {
    /// Corpus as a QQL filter. `None` = global collection statistics.
    pub corpus: Option<FilterExpr>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PayloadSelector {
    All,
    None,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorSelector {
    All,
    None,
    Names(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QueryOutput {
    pub payload: Option<PayloadSelector>,
    pub vectors: Option<VectorSelector>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupSpec {
    pub field: String,
    pub size: Option<u64>,
    pub lookup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PageSpec {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cte {
    pub name: String,
    pub query: Box<QueryStmt>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QueryStmt {
    pub ctes: Vec<Cte>,
    pub collection: QueryCollection,
    pub expression: QueryExpr,
    pub filter: Option<Box<FilterExpr>>,
    pub params: Option<SearchParams>,
    pub score_threshold: Option<f64>,
    pub group: Option<GroupSpec>,
    pub output: QueryOutput,
    pub page: PageSpec,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScrollStmt {
    pub collection: String,
    pub limit: u64,
    pub filter: Option<Box<FilterExpr>>,
    pub after: Option<PointId>,
    pub shard_key: Option<String>,
    /// Optional `WITH VECTOR` selector. Defaults to no vectors when `None`.
    pub with_vector: Option<VectorSelector>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbedKind {
    Dense {
        model: Option<String>,
    },
    Sparse {
        model: Option<String>,
    },
    /// Multivector / ColBERT bag (`embed_multi` → MultiDense).
    Multi {
        model: Option<String>,
    },
    /// Image / CLIP vision path or URL → dense vector (`embed_image`).
    Image {
        model: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EmbedDirective {
    pub source_field: String,
    pub target_vector: String,
    pub kind: EmbedKind,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbeddingSpec {
    Dense {
        model: Option<String>,
        vector: Option<String>,
        field: Option<String>,
    },
    Sparse {
        model: Option<String>,
        vector: Option<String>,
        field: Option<String>,
    },
    Hybrid {
        dense_model: Option<String>,
        dense_vector: Option<String>,
        dense_field: Option<String>,
        sparse_model: Option<String>,
        sparse_vector: Option<String>,
        sparse_field: Option<String>,
    },
    /// Multivector / ColBERT: text → bag of token vectors for a named multi slot.
    MultiVector {
        model: Option<String>,
        vector: Option<String>,
        field: Option<String>,
    },
    /// Image / CLIP vision: payload field holds a path or URL → dense vector.
    Image {
        model: Option<String>,
        vector: Option<String>,
        field: Option<String>,
    },
    /// Combined specs (e.g. DENSE + SPARSE + MULTI VECTOR colbert).
    Multi(Vec<EmbeddingSpec>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpsertPoint {
    pub id: PointId,
    pub vectors: Option<PointVectors>,
    pub payload: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpsertStmt {
    pub collection: String,
    pub points: Vec<UpsertPoint>,
    pub embedding: Option<EmbeddingSpec>,
    pub embed: Vec<EmbedDirective>,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorDistance {
    Cosine,
    Dot,
    Euclid,
    Manhattan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MultivectorComparator {
    MaxSim,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultivectorConfig {
    pub comparator: MultivectorComparator,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorDef {
    pub name: String,
    pub size: u64,
    pub distance: VectorDistance,
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    pub quantization: Option<Box<QuantizationConfig>>,
    pub multivector: Option<MultivectorConfig>,
    pub vectors: Option<Box<VectorsConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SparseIndexConfig {
    pub full_scan_threshold: Option<u64>,
    pub on_disk: Option<bool>,
    /// Storage datatype (`float32` / `float16` / `uint8`). Turbo4 is rejected.
    pub datatype: Option<VectorDatatype>,
    /// Memory placement of the index.
    pub memory: Option<MemoryPlacement>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SparseVectorDef {
    pub name: String,
    pub index: Option<Box<SparseIndexConfig>>,
    pub modifier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QuantizationType {
    Scalar,
    Binary,
    Product,
    Turbo,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationConfig {
    pub qtype: QuantizationType,
    pub always_ram: bool,
    pub quantile: Option<f64>,
    pub bits: Option<f64>,
    pub compression: Option<String>,
    pub encoding: Option<String>,
    pub query_encoding: Option<String>,
    /// Memory placement of quantized vectors.
    pub memory: Option<MemoryPlacement>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationUpdate {
    pub disabled: bool,
    pub config: Option<Box<QuantizationConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HnswRuntimeConfig {
    pub m: Option<u64>,
    pub ef_construct: Option<u64>,
    pub full_scan_threshold: Option<u64>,
    pub max_indexing_threads: Option<u64>,
    pub on_disk: Option<bool>,
    pub payload_m: Option<u64>,
    pub inline_storage: Option<bool>,
    /// Memory placement of the HNSW graph.
    pub memory: Option<MemoryPlacement>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorsConfig {
    pub on_disk: Option<bool>,
    /// Memory placement of the original vector storage.
    pub memory: Option<MemoryPlacement>,
    /// Storage datatype for dense vectors.
    pub datatype: Option<VectorDatatype>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OptimizationThreads {
    pub auto_: bool,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OptimizersRuntimeConfig {
    pub deleted_threshold: Option<f64>,
    pub vacuum_min_vector_number: Option<u64>,
    pub default_segment_number: Option<u64>,
    pub max_segment_size: Option<u64>,
    pub memmap_threshold: Option<u64>,
    pub indexing_threshold: Option<u64>,
    pub flush_interval_sec: Option<u64>,
    pub max_optimization_threads: Option<OptimizationThreads>,
    pub prevent_unoptimized: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CollectionParamsConfig {
    pub replication_factor: Option<u64>,
    pub write_consistency_factor: Option<u64>,
    pub read_fan_out_factor: Option<u64>,
    pub read_fan_out_delay_ms: Option<u64>,
    pub on_disk_payload: Option<bool>,
    /// Memory placement of the payload storage (`Cold` / `Cached` only; never `Pinned`).
    pub payload_memory: Option<MemoryPlacement>,
    pub shard_number: Option<u64>,
    pub sharding_method: Option<String>,
    pub shard_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CollectionConfig {
    pub vectors: Option<Box<VectorsConfig>>,
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    pub optimizers: Option<Box<OptimizersRuntimeConfig>>,
    pub params: Option<Box<CollectionParamsConfig>>,
    pub quantization: Option<Box<QuantizationConfig>>,
    pub quantization_update: Option<Box<QuantizationUpdate>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CollectionMode {
    Dense {
        model: Option<String>,
    },
    Hybrid {
        dense_vector: Option<String>,
        sparse_vector: Option<String>,
    },
    Rerank,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClearPayloadStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteVectorStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub vector_names: Vec<String>,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreateCollectionStmt {
    pub collection: String,
    pub mode: CollectionMode,
    pub vectors: Vec<VectorDef>,
    pub sparse_vectors: Vec<SparseVectorDef>,
    pub config: Option<Box<CollectionConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlterCollectionStmt {
    pub collection: String,
    pub config: Option<Box<CollectionConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DropCollectionStmt {
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreateIndexStmt {
    pub collection: String,
    pub field: String,
    pub field_type: String,
    pub options: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DropIndexStmt {
    pub collection: String,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CountStmt {
    pub collection: QueryCollection,
    pub filter: Option<Box<FilterExpr>>,
    pub shard_key: Option<String>,
    pub exact: Option<bool>,
}

/// In-database categorical facet aggregation statement (`FACET <key> FROM <collection>`).
///
/// Compiles to Qdrant's `/collections/{collection}/facet` endpoint, returning hit counts
/// per unique value for a payload field without retrieving full point records.
///
/// # Supported clauses
/// - `WHERE`: Optional filter restricting candidate points.
/// - `LIMIT`: Maximum number of distinct facet values to return.
/// - `EXACT`: Whether to compute exact distributed counts across shards.
/// - `SHARD`: Target shard key for tenant-partitioned collections.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FacetStmt {
    /// Payload field name to aggregate values for.
    pub key: String,
    /// Target collection.
    pub collection: QueryCollection,
    /// Optional point filter scoping the aggregation.
    pub filter: Option<Box<FilterExpr>>,
    /// Maximum number of unique facet hits to return.
    pub limit: Option<u64>,
    /// Whether to compute exact distributed counts across shards.
    pub exact: Option<bool>,
    /// Optional shard key partition routing.
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreateShardKeyStmt {
    pub collection: String,
    pub shard_key: String,
    pub shards_number: Option<u64>,
    pub replication_factor: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DropShardKeyStmt {
    pub collection: String,
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
    pub config: Vec<(String, Value)>,
    pub wait: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointSelector {
    Id(PointId),
    Ids(Vec<PointId>),
    Filter(Box<FilterExpr>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdateVectorStmt {
    pub collection: String,
    pub point_id: PointId,
    pub vector: VectorValue,
    pub vector_name: Option<String>,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeletePayloadStmt {
    pub collection: String,
    pub keys: Vec<String>,
    pub selector: PointSelector,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdatePayloadStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub payload: Vec<(String, Value)>,
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Query(Box<QueryStmt>),
    Scroll(Box<ScrollStmt>),
    Upsert(Box<UpsertStmt>),
    CreateCollection(Box<CreateCollectionStmt>),
    CreateIndex(Box<CreateIndexStmt>),
    DropIndex(Box<DropIndexStmt>),
    CreateShardKey(Box<CreateShardKeyStmt>),
    DropShardKey(Box<DropShardKeyStmt>),
    AlterCollection(Box<AlterCollectionStmt>),
    DropCollection(Box<DropCollectionStmt>),
    ShowCollections,
    ShowCollection(String),
    ShowShardKeys(String),
    Delete(Box<DeleteStmt>),
    ClearPayload(Box<ClearPayloadStmt>),
    DeletePayload(Box<DeletePayloadStmt>),
    DeleteVector(Box<DeleteVectorStmt>),
    UpdateVector(Box<UpdateVectorStmt>),
    UpdatePayload(Box<UpdatePayloadStmt>),
    Count(Box<CountStmt>),
    Facet(Box<FacetStmt>),
    ShowQuotas,
    SetQuota(Box<SetQuotaStmt>),
}

#[cfg(feature = "serde")]
impl serde::Serialize for Stmt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Stmt::Query(s) => serializer.serialize_newtype_variant("Stmt", 0, "Query", s),
            Stmt::Scroll(s) => serializer.serialize_newtype_variant("Stmt", 1, "Scroll", s),
            Stmt::Upsert(s) => serializer.serialize_newtype_variant("Stmt", 2, "Upsert", s),
            Stmt::CreateCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 3, "CreateCollection", s)
            }
            Stmt::CreateIndex(s) => {
                serializer.serialize_newtype_variant("Stmt", 4, "CreateIndex", s)
            }
            Stmt::DropIndex(s) => serializer.serialize_newtype_variant("Stmt", 5, "DropIndex", s),
            Stmt::CreateShardKey(s) => {
                serializer.serialize_newtype_variant("Stmt", 6, "CreateShardKey", s)
            }
            Stmt::DropShardKey(s) => {
                serializer.serialize_newtype_variant("Stmt", 7, "DropShardKey", s)
            }
            Stmt::AlterCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 8, "AlterCollection", s)
            }
            Stmt::DropCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 9, "DropCollection", s)
            }
            // Unit variant. The serialized form is the empty-object tag
            // `{"ShowCollections": {}}` (kept for backward compatibility with
            // consumers that already emit that shape). The manual
            // `Deserialize` accepts both this form and the derived string
            // form `"ShowCollections"`, so serde round-trips.
            Stmt::ShowCollections => {
                let mut map = serializer.serialize_map(Some(1))?;
                let empty = std::collections::BTreeMap::<String, String>::new();
                map.serialize_entry("ShowCollections", &empty)?;
                map.end()
            }
            Stmt::ShowCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 11, "ShowCollection", s)
            }
            Stmt::ShowShardKeys(s) => {
                serializer.serialize_newtype_variant("Stmt", 12, "ShowShardKeys", s)
            }
            Stmt::Delete(s) => serializer.serialize_newtype_variant("Stmt", 13, "Delete", s),
            Stmt::ClearPayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 14, "ClearPayload", s)
            }
            Stmt::DeletePayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 15, "DeletePayload", s)
            }
            Stmt::DeleteVector(s) => {
                serializer.serialize_newtype_variant("Stmt", 16, "DeleteVector", s)
            }
            Stmt::UpdateVector(s) => {
                serializer.serialize_newtype_variant("Stmt", 17, "UpdateVector", s)
            }
            Stmt::UpdatePayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 18, "UpdatePayload", s)
            }
            Stmt::Count(s) => serializer.serialize_newtype_variant("Stmt", 19, "Count", s),
            Stmt::Facet(s) => serializer.serialize_newtype_variant("Stmt", 20, "Facet", s),
            Stmt::ShowQuotas => {
                let mut map = serializer.serialize_map(Some(1))?;
                let empty = std::collections::BTreeMap::<String, String>::new();
                map.serialize_entry("ShowQuotas", &empty)?;
                map.end()
            }
            Stmt::SetQuota(s) => serializer.serialize_newtype_variant("Stmt", 22, "SetQuota", s),
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Stmt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use core::fmt;
        use serde::de::{Error as _, IgnoredAny, MapAccess, Visitor};

        struct StmtVisitor;

        impl<'de> Visitor<'de> for StmtVisitor {
            type Value = Stmt;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an externally tagged QQL statement")
            }

            /// Derived externally-tagged form of the unit variant.
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value == "ShowCollections" {
                    Ok(Stmt::ShowCollections)
                } else {
                    Err(E::unknown_variant(value, &["ShowCollections"]))
                }
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let key = map
                    .next_key::<alloc::string::String>()?
                    .ok_or_else(|| A::Error::custom("expected a statement tag"))?;
                let stmt = match key.as_str() {
                    "Query" => Stmt::Query(map.next_value()?),
                    "Scroll" => Stmt::Scroll(map.next_value()?),
                    "Upsert" => Stmt::Upsert(map.next_value()?),
                    "CreateCollection" => Stmt::CreateCollection(map.next_value()?),
                    "CreateIndex" => Stmt::CreateIndex(map.next_value()?),
                    "DropIndex" => Stmt::DropIndex(map.next_value()?),
                    "CreateShardKey" => Stmt::CreateShardKey(map.next_value()?),
                    "DropShardKey" => Stmt::DropShardKey(map.next_value()?),
                    "AlterCollection" => Stmt::AlterCollection(map.next_value()?),
                    "DropCollection" => Stmt::DropCollection(map.next_value()?),
                    // Canonical serialized form (`{"ShowCollections": {}}`);
                    // the payload is ignored, mirroring the derived impl's
                    // permissive unit-variant handling.
                    "ShowCollections" => {
                        map.next_value::<IgnoredAny>()?;
                        Stmt::ShowCollections
                    }
                    "ShowQuotas" => {
                        map.next_value::<IgnoredAny>()?;
                        Stmt::ShowQuotas
                    }
                    "ShowCollection" => Stmt::ShowCollection(map.next_value()?),
                    "ShowShardKeys" => Stmt::ShowShardKeys(map.next_value()?),
                    "Delete" => Stmt::Delete(map.next_value()?),
                    "ClearPayload" => Stmt::ClearPayload(map.next_value()?),
                    "DeletePayload" => Stmt::DeletePayload(map.next_value()?),
                    "DeleteVector" => Stmt::DeleteVector(map.next_value()?),
                    "UpdateVector" => Stmt::UpdateVector(map.next_value()?),
                    "UpdatePayload" => Stmt::UpdatePayload(map.next_value()?),
                    "Count" => Stmt::Count(map.next_value()?),
                    "Facet" => Stmt::Facet(map.next_value()?),
                    "SetQuota" => Stmt::SetQuota(map.next_value()?),
                    _ => {
                        return Err(A::Error::unknown_variant(
                            &key,
                            &[
                                "Query",
                                "Scroll",
                                "Upsert",
                                "CreateCollection",
                                "CreateIndex",
                                "DropIndex",
                                "CreateShardKey",
                                "DropShardKey",
                                "AlterCollection",
                                "DropCollection",
                                "ShowCollections",
                                "ShowCollection",
                                "ShowShardKeys",
                                "Delete",
                                "ClearPayload",
                                "DeletePayload",
                                "DeleteVector",
                                "UpdateVector",
                                "UpdatePayload",
                                "Count",
                                "Facet",
                                "ShowQuotas",
                                "SetQuota",
                            ],
                        ));
                    }
                };
                if map.next_key::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::custom("duplicate statement tag"));
                }
                Ok(stmt)
            }
        }

        deserializer.deserialize_any(StmtVisitor)
    }
}
