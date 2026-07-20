use super::{FilterExpr, FormulaExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum QueryMode {
    Nearest,
    Recommend,
    Context,
    Discover,
    OrderBy,
    Sample,
    RelevanceFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum QueryType {
    Dense,
    Sparse,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ContextPair {
    pub positive: Value,
    pub negative: Value,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FeedbackItem {
    pub example: Value,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FeedbackStrategyType {
    Naive,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FeedbackStrategy {
    pub strategy_type: FeedbackStrategyType,
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CTE {
    pub name: String,
    pub stmt: Box<QueryStmt>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PrefetchRef {
    pub cte_name: String,
    pub filter: Option<Box<FilterExpr>>,
    pub score_threshold: Option<f64>,
    pub lookup_from: Option<String>,
    pub lookup_vector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SearchWith {
    pub hnsw_ef: u64,
    pub exact: bool,
    pub acorn: bool,
    pub indexed_only: bool,
    pub quantization: Option<Box<QuantizationSearchWith>>,
    pub mmr_diversity: Option<f64>,
    pub mmr_candidates: Option<u64>,
    pub rrf_k: Option<u64>,
    pub rrf_weights: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QuantizationSearchWith {
    pub ignore: Option<bool>,
    pub rescore: Option<bool>,
    pub oversampling: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PayloadSelector {
    pub enable: Option<bool>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct VectorsSelector {
    pub enable: Option<bool>,
    pub vectors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QueryStmt {
    pub collection: Option<String>,
    pub mode: QueryMode,
    pub query_type: QueryType,
    pub query_text: Option<String>,
    pub query_id: Option<Value>,
    pub raw_vector: Vec<f64>,
    pub positive_ids: Vec<Value>,
    pub negative_ids: Vec<Value>,
    pub context_pairs: Vec<ContextPair>,
    pub target: Option<Value>,
    pub order_by_field: Option<String>,
    pub order_by_asc: Option<bool>,
    pub limit: i64,
    pub offset: i64,
    pub score_threshold: Option<f64>,
    pub strategy: Option<String>,
    pub query_filter: Option<Box<FilterExpr>>,
    pub group_by: Option<String>,
    pub group_size: Option<i64>,
    pub with_clause: Option<Box<SearchWith>>,
    pub with_payload: Option<Box<PayloadSelector>>,
    pub with_vectors: Option<Box<VectorsSelector>>,
    pub lookup_from: Option<String>,
    pub lookup_vector: Option<String>,
    pub with_lookup_collection: Option<String>,
    pub using_: Option<String>,
    pub model: Option<String>,
    pub ctes: Vec<CTE>,
    pub prefetch_refs: Vec<PrefetchRef>,
    pub fusion_type: Option<String>,
    pub rerank: bool,
    pub rerank_model: Option<String>,
    pub formula: Option<Box<FormulaExpr>>,
    pub formula_defaults: Vec<(String, Value)>,
    pub feedback_target: Option<Value>,
    pub feedback_items: Vec<FeedbackItem>,
    pub feedback_strategy: Option<Box<FeedbackStrategy>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SelectStmt {
    pub collection: String,
    pub point_id: Value,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ScrollStmt {
    pub collection: String,
    pub limit: i64,
    pub query_filter: Option<Box<FilterExpr>>,
    pub after: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EmbedDirective {
    pub source_field: String,
    pub target_vector: String,
    pub model: Option<String>,
    pub sparse_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct InsertStmt {
    pub collection: String,
    pub values_list: Vec<Vec<(String, Value)>>,
    pub model: Option<String>,
    pub hybrid: bool,
    pub sparse_model: Option<String>,
    pub dense_vector: Option<String>,
    pub sparse_vector: Option<String>,
    pub embed_directives: Vec<EmbedDirective>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum VectorDistance {
    Cosine,
    Dot,
    Euclid,
    Manhattan,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MultivectorConfig {
    pub comparator: &'static str,
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
pub struct CreateCollectionStmt {
    pub collection: String,
    pub hybrid: bool,
    pub rerank: bool,
    pub model: Option<String>,
    pub dense_vector: Option<String>,
    pub sparse_vector: Option<String>,
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
pub struct DeleteStmt {
    pub collection: String,
    pub point_id: Option<Value>,
    pub field: Option<String>,
    pub value: Option<Value>,
    pub query_filter: Option<Box<FilterExpr>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpdateVectorStmt {
    pub collection: String,
    pub point_id: Value,
    pub vector: Vec<f32>,
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpdatePayloadStmt {
    pub collection: String,
    pub point_id: Option<Value>,
    pub query_filter: Option<Box<FilterExpr>>,
    pub payload: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Stmt {
    Query(Box<QueryStmt>),
    Select(Box<SelectStmt>),
    Scroll(Box<ScrollStmt>),
    Insert(Box<InsertStmt>),
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
