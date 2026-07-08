use super::{FilterExpr, FormulaExpr, Value};
use alloc::borrow::Cow;
use alloc::boxed::Box;
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
pub struct ContextPair<'a> {
    pub positive: Value<'a>,
    pub negative: Value<'a>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FeedbackItem<'a> {
    pub example: Value<'a>,
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
pub struct CTE<'a> {
    pub name: Cow<'a, str>,
    pub stmt: Box<QueryStmt<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PrefetchRef<'a> {
    pub cte_name: Cow<'a, str>,
    pub filter: Option<Box<FilterExpr<'a>>>,
    pub score_threshold: Option<f64>,
    pub lookup_from: Option<&'a str>,
    pub lookup_vector: Option<&'a str>,
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
pub struct PayloadSelector<'a> {
    pub enable: Option<bool>,
    pub include: Vec<&'a str>,
    pub exclude: Vec<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct VectorsSelector<'a> {
    pub enable: Option<bool>,
    pub vectors: Vec<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QueryStmt<'a> {
    pub collection: Option<&'a str>,
    pub mode: QueryMode,
    pub query_type: QueryType,
    pub query_text: Option<&'a str>,
    pub query_id: Option<Value<'a>>,
    pub raw_vector: Vec<f64>,
    pub positive_ids: Vec<Value<'a>>,
    pub negative_ids: Vec<Value<'a>>,
    pub context_pairs: Vec<ContextPair<'a>>,
    pub target: Option<Value<'a>>,
    pub order_by_field: Option<&'a str>,
    pub order_by_asc: Option<bool>,
    pub limit: i64,
    pub offset: i64,
    pub score_threshold: Option<f64>,
    pub strategy: Option<&'a str>,
    pub query_filter: Option<Box<FilterExpr<'a>>>,
    pub group_by: Option<&'a str>,
    pub group_size: Option<i64>,
    pub with_clause: Option<Box<SearchWith>>,
    pub with_payload: Option<Box<PayloadSelector<'a>>>,
    pub with_vectors: Option<Box<VectorsSelector<'a>>>,
    pub lookup_from: Option<&'a str>,
    pub lookup_vector: Option<&'a str>,
    pub with_lookup_collection: Option<&'a str>,
    pub using_: Option<&'a str>,
    pub model: Option<&'a str>,
    pub ctes: Vec<CTE<'a>>,
    pub prefetch_refs: Vec<PrefetchRef<'a>>,
    pub fusion_type: Option<&'a str>,
    pub rerank: bool,
    pub rerank_model: Option<&'a str>,
    pub formula: Option<Box<FormulaExpr<'a>>>,
    pub formula_defaults: Vec<(&'a str, Value<'a>)>,
    pub feedback_target: Option<Value<'a>>,
    pub feedback_items: Vec<FeedbackItem<'a>>,
    pub feedback_strategy: Option<Box<FeedbackStrategy>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SelectStmt<'a> {
    pub collection: &'a str,
    pub point_id: Value<'a>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ScrollStmt<'a> {
    pub collection: &'a str,
    pub limit: i64,
    pub query_filter: Option<Box<FilterExpr<'a>>>,
    pub after: Option<Value<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EmbedDirective<'a> {
    pub source_field: &'a str,
    pub target_vector: &'a str,
    pub model: Option<&'a str>,
    pub sparse_model: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct InsertStmt<'a> {
    pub collection: &'a str,
    pub values_list: Vec<Vec<(&'a str, Value<'a>)>>,
    pub model: Option<&'a str>,
    pub hybrid: bool,
    pub sparse_model: Option<&'a str>,
    pub dense_vector: Option<&'a str>,
    pub sparse_vector: Option<&'a str>,
    pub embed_directives: Vec<EmbedDirective<'a>>,
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
pub struct VectorDef<'a> {
    pub name: &'a str,
    pub size: u64,
    pub distance: VectorDistance,
    pub hnsw: Option<Box<HnswRuntimeConfig>>,
    pub quantization: Option<Box<QuantizationConfig>>,
    pub multivector: Option<MultivectorConfig>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SparseVectorDef<'a> {
    pub name: &'a str,
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
pub struct CreateCollectionStmt<'a> {
    pub collection: &'a str,
    pub hybrid: bool,
    pub rerank: bool,
    pub model: Option<&'a str>,
    pub dense_vector: Option<&'a str>,
    pub sparse_vector: Option<&'a str>,
    pub vectors: Vec<VectorDef<'a>>,
    pub sparse_vectors: Vec<SparseVectorDef<'a>>,
    pub config: Option<Box<CollectionConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct AlterCollectionStmt<'a> {
    pub collection: &'a str,
    pub config: Option<Box<CollectionConfig>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DropCollectionStmt<'a> {
    pub collection: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CreateIndexStmt<'a> {
    pub collection: &'a str,
    pub field: &'a str,
    pub field_type: Cow<'a, str>,
    pub options: Vec<(&'a str, Value<'a>)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DeleteStmt<'a> {
    pub collection: &'a str,
    pub point_id: Option<Value<'a>>,
    pub field: Option<&'a str>,
    pub value: Option<Value<'a>>,
    pub query_filter: Option<Box<FilterExpr<'a>>>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpdateVectorStmt<'a> {
    pub collection: &'a str,
    pub point_id: Value<'a>,
    pub vector: Vec<f32>,
    pub vector_name: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UpdatePayloadStmt<'a> {
    pub collection: &'a str,
    pub point_id: Option<Value<'a>>,
    pub query_filter: Option<Box<FilterExpr<'a>>>,
    pub payload: Vec<(&'a str, Value<'a>)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Stmt<'a> {
    Query(Box<QueryStmt<'a>>),
    Select(Box<SelectStmt<'a>>),
    Scroll(Box<ScrollStmt<'a>>),
    Insert(Box<InsertStmt<'a>>),
    CreateCollection(Box<CreateCollectionStmt<'a>>),
    CreateIndex(Box<CreateIndexStmt<'a>>),
    AlterCollection(Box<AlterCollectionStmt<'a>>),
    DropCollection(Box<DropCollectionStmt<'a>>),
    ShowCollections,
    ShowCollection(&'a str),
    Delete(Box<DeleteStmt<'a>>),
    UpdateVector(Box<UpdateVectorStmt<'a>>),
    UpdatePayload(Box<UpdatePayloadStmt<'a>>),
}
