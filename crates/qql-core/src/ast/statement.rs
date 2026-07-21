use super::{FilterExpr, FormulaExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PointId {
    Number(u64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum VectorValue {
    Dense(Vec<f32>),
    Sparse { indices: Vec<u32>, values: Vec<f32> },
    MultiDense(Vec<Vec<f32>>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PointVectors {
    Unnamed(VectorValue),
    Named(Vec<(String, VectorValue)>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum QueryInput {
    Text { text: String, model: Option<String> },
    Vector(VectorValue),
    Point(PointId),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ContextPair {
    pub positive: QueryInput,
    pub negative: QueryInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum RecommendStrategy {
    AverageVector,
    BestScore,
    SumScores,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FeedbackItem {
    pub example: QueryInput,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FeedbackStrategy {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FusionMethod {
    Rrf,
    Dbsf,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum QueryCollection {
    Explicit(String),
    Inherited,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PrefetchSource {
    Cte(String),
    Query(Box<QueryStmt>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct LookupSpec {
    pub collection: String,
    pub vector: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Prefetch {
    pub source: PrefetchSource,
    pub filter: Option<Box<FilterExpr>>,
    pub score_threshold: Option<f64>,
    pub lookup: Option<LookupSpec>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum QueryExpr {
    Points {
        ids: Vec<PointId>,
    },
    Nearest {
        input: QueryInput,
        using: Option<String>,
        prefetch: Vec<Prefetch>,
    },
    Recommend {
        positive: Vec<QueryInput>,
        negative: Vec<QueryInput>,
        strategy: Option<RecommendStrategy>,
        using: Option<String>,
        prefetch: Vec<Prefetch>,
    },
    Context {
        pairs: Vec<ContextPair>,
        using: Option<String>,
        prefetch: Vec<Prefetch>,
    },
    Discover {
        target: QueryInput,
        context: Vec<ContextPair>,
        using: Option<String>,
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
        using: Option<String>,
        prefetch: Vec<Prefetch>,
    },
    Mmr {
        input: QueryInput,
        diversity: f64,
        candidates: u64,
        using: Option<String>,
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
        using: String,
        prefetch: Vec<Prefetch>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QuantizationSearchParams {
    pub ignore: Option<bool>,
    pub rescore: Option<bool>,
    pub oversampling: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SearchParams {
    pub hnsw_ef: Option<u64>,
    pub exact: Option<bool>,
    pub acorn: Option<bool>,
    pub indexed_only: Option<bool>,
    pub quantization: Option<QuantizationSearchParams>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PayloadSelector {
    All,
    None,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum VectorSelector {
    All,
    None,
    Names(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QueryOutput {
    pub payload: Option<PayloadSelector>,
    pub vectors: Option<VectorSelector>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GroupSpec {
    pub field: String,
    pub size: Option<u64>,
    pub lookup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PageSpec {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Cte {
    pub name: String,
    pub query: Box<QueryStmt>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ScrollStmt {
    pub collection: String,
    pub limit: u64,
    pub filter: Option<Box<FilterExpr>>,
    pub after: Option<PointId>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum EmbedKind {
    Dense { model: Option<String> },
    Sparse { model: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EmbedDirective {
    pub source_field: String,
    pub target_vector: String,
    pub kind: EmbedKind,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum EmbeddingSpec {
    Dense {
        model: Option<String>,
        vector: Option<String>,
    },
    Hybrid {
        dense_model: Option<String>,
        dense_vector: Option<String>,
        sparse_model: Option<String>,
        sparse_vector: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpsertPoint {
    pub id: PointId,
    pub vectors: Option<PointVectors>,
    pub payload: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpsertStmt {
    pub collection: String,
    pub points: Vec<UpsertPoint>,
    pub embedding: Option<EmbeddingSpec>,
    pub embed: Vec<EmbedDirective>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum VectorDistance {
    Cosine,
    Dot,
    Euclid,
    Manhattan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum MultivectorComparator {
    MaxSim,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MultivectorConfig {
    pub comparator: MultivectorComparator,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct VectorDef {
    pub name: String,
    pub size: u64,
    pub distance: VectorDistance,
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    pub quantization: Option<Box<QuantizationConfig>>,
    pub multivector: Option<MultivectorConfig>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SparseVectorDef {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum QuantizationType {
    Scalar,
    Binary,
    Product,
    Turbo,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QuantizationConfig {
    pub qtype: QuantizationType,
    pub always_ram: bool,
    pub quantile: Option<f64>,
    pub turbo_bits: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QuantizationUpdate {
    pub disabled: bool,
    pub config: Option<Box<QuantizationConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct HnswRuntimeConfig {
    pub m: Option<u64>,
    pub ef_construct: Option<u64>,
    pub full_scan_threshold: Option<u64>,
    pub max_indexing_threads: Option<u64>,
    pub on_disk: Option<bool>,
    pub payload_m: Option<u64>,
    pub inline_storage: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct VectorsConfig {
    pub on_disk: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct OptimizationThreads {
    pub auto_: bool,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CollectionParamsConfig {
    pub replication_factor: Option<u64>,
    pub write_consistency_factor: Option<u64>,
    pub read_fan_out_factor: Option<u64>,
    pub read_fan_out_delay_ms: Option<u64>,
    pub on_disk_payload: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CollectionConfig {
    pub vectors: Option<Box<VectorsConfig>>,
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    pub optimizers: Option<Box<OptimizersRuntimeConfig>>,
    pub params: Option<Box<CollectionParamsConfig>>,
    pub quantization: Option<Box<QuantizationConfig>>,
    pub quantization_update: Option<Box<QuantizationUpdate>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CreateCollectionStmt {
    pub collection: String,
    pub mode: CollectionMode,
    pub vectors: Vec<VectorDef>,
    pub sparse_vectors: Vec<SparseVectorDef>,
    pub config: Option<Box<CollectionConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct AlterCollectionStmt {
    pub collection: String,
    pub config: Option<Box<CollectionConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DropCollectionStmt {
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CreateIndexStmt {
    pub collection: String,
    pub field: String,
    pub field_type: String,
    pub options: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PointSelector {
    Id(PointId),
    Ids(Vec<PointId>),
    Filter(Box<FilterExpr>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DeleteStmt {
    pub collection: String,
    pub selector: PointSelector,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpdateVectorStmt {
    pub collection: String,
    pub point_id: PointId,
    pub vector: VectorValue,
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpdatePayloadStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub payload: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Stmt {
    Query(Box<QueryStmt>),
    Scroll(Box<ScrollStmt>),
    Upsert(Box<UpsertStmt>),
    CreateCollection(Box<CreateCollectionStmt>),
    CreateIndex(Box<CreateIndexStmt>),
    AlterCollection(Box<AlterCollectionStmt>),
    DropCollection(Box<DropCollectionStmt>),
    ShowCollections,
    ShowCollection(String),
    Delete(Box<DeleteStmt>),
    UpdateVector(Box<UpdateVectorStmt>),
    UpdatePayload(Box<UpdatePayloadStmt>),
}
