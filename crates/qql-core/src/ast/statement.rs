use super::{FilterExpr, FormulaExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

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
    Text { text: String, model: Option<String> },
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
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationSearchParams {
    pub ignore: Option<bool>,
    pub rescore: Option<bool>,
    pub oversampling: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchParams {
    pub hnsw_ef: Option<u64>,
    pub exact: Option<bool>,
    pub acorn: Option<bool>,
    pub indexed_only: Option<bool>,
    pub quantization: Option<QuantizationSearchParams>,
    pub rrf_k: Option<u64>,
    pub rrf_weights: Option<Vec<f64>>,
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
    Dense { model: Option<String> },
    Sparse { model: Option<String> },
    /// Multivector / ColBERT bag (`embed_multi` → MultiDense).
    Multi { model: Option<String> },
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
    pub datatype: Option<String>,
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
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorsConfig {
    pub on_disk: Option<bool>,
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
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteVectorStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub vector_names: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdatePayloadStmt {
    pub collection: String,
    pub selector: PointSelector,
    pub payload: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
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
    DeleteVector(Box<DeleteVectorStmt>),
    UpdateVector(Box<UpdateVectorStmt>),
    UpdatePayload(Box<UpdatePayloadStmt>),
    Count(Box<CountStmt>),
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
            Stmt::DeleteVector(s) => {
                serializer.serialize_newtype_variant("Stmt", 15, "DeleteVector", s)
            }
            Stmt::UpdateVector(s) => {
                serializer.serialize_newtype_variant("Stmt", 16, "UpdateVector", s)
            }
            Stmt::UpdatePayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 17, "UpdatePayload", s)
            }
            Stmt::Count(s) => serializer.serialize_newtype_variant("Stmt", 18, "Count", s),
        }
    }
}
