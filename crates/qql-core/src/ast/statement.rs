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
    /// Prefer disk; load on demand.
    Cold,
    /// Cache in RAM when hot.
    Cached,
    /// Keep in RAM (invalid for payload storage).
    Pinned,
}

impl MemoryPlacement {
    /// Canonical lowercase keyword for this placement.
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
    /// 32-bit IEEE float (default dense storage).
    Float32,
    /// 16-bit IEEE half float.
    Float16,
    /// 8-bit unsigned integer.
    Uint8,
    /// 4-bit Turbo quantized storage (dense vectors only).
    Turbo4,
}

impl VectorDatatype {
    /// Canonical lowercase keyword for this datatype.
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

/// Point identifier: unsigned integer or unique string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointId {
    /// Unsigned 64-bit integer ID.
    Number(u64),
    /// Arbitrary unique string ID.
    String(String),
}

/// A vector value: dense, sparse, or multivector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorValue {
    /// Single dense vector.
    Dense(Vec<f32>),
    /// Sparse vector — `indices` positions paired with `values` weights.
    Sparse {
        /// Row positions of the non-zero entries.
        indices: Vec<u32>,
        /// Weights at the indexed rows.
        values: Vec<f32>,
    },
    /// Multivector bag of dense vectors (ColBERT-style MaxSim).
    MultiDense(Vec<Vec<f32>>),
}

/// Vector payload attached to an upsert point.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointVectors {
    /// One unnamed vector value.
    Unnamed(VectorValue),
    /// Vector values keyed by vector name.
    Named(Vec<(String, VectorValue)>),
}

/// Query input: embeddable text/image, pre-computed vector, or point reference.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryInput {
    /// Text to embed (`TEXT '…' [MODEL '…']`).
    Text {
        /// The text to embed.
        text: String,
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Image path or URL for dense embedding (CLIP vision, etc.).
    /// Resolved to [`VectorValue::Dense`] before plan/dispatch.
    Image {
        /// Image path or URL.
        source: String,
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Pre-computed vector value used as-is.
    Vector(VectorValue),
    /// Reference point — use an existing point's vector as the input.
    Point(PointId),
}

/// Explicit `AS` role for a `USING` vector target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorKind {
    /// Single dense embedding.
    Dense,
    /// Sparse (e.g. BM25) embedding.
    Sparse,
}

/// `USING <name> [AS <kind>]` resolution target.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorTarget {
    /// Named vector to embed into.
    pub name: String,
    /// Explicit `AS DENSE` / `AS SPARSE` role; schema-resolved when `None`.
    pub kind: Option<VectorKind>,
    /// Multivector (ColBERT-style) dense target. Filled at parse only via
    /// `AS MULTI`, or at execution prep from collection schema
    /// (`multivector_config`). Not a third `VectorKind` — still dense.
    #[cfg_attr(feature = "serde", serde(default))]
    pub multi: bool,
}

/// Maximal marginal relevance settings (`MMR … DIVERSITY … CANDIDATES …`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MmrConfig {
    /// Relevance-to-diversity trade-off in `[0, 1]`.
    pub diversity: f64,
    /// Size of the candidate pool considered by the MMR pass.
    pub candidates: u64,
}

/// One positive/negative example pair.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContextPair {
    /// Example the results should be similar to.
    pub positive: QueryInput,
    /// Example the results should move away from.
    pub negative: QueryInput,
}

/// `RECOMMEND … STRATEGY` scoring strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RecommendStrategy {
    /// `average_vector` — score against the averaged example vectors.
    AverageVector,
    /// `best_score` — score against the most similar example.
    BestScore,
    /// `sum_scores` — sum similarity across all examples.
    SumScores,
}

/// One relevance feedback example with its weight.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FeedbackItem {
    /// Feedback example input.
    pub example: QueryInput,
    /// Weight applied to this example's vector.
    pub score: f64,
}

/// `STRATEGY NAIVE (a = …, b = …, c = …)` relevance feedback weights.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FeedbackStrategy {
    /// Weight of the target vector.
    pub a: f64,
    /// Weight of the positive feedback examples.
    pub b: f64,
    /// Weight of the negative feedback examples.
    pub c: f64,
}

/// `ORDER BY` sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OrderDirection {
    /// `ASC` — ascending (the default).
    Asc,
    /// `DESC` — descending.
    Desc,
}

/// Rank/score fusion method over prefetch stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FusionMethod {
    /// `RRF` — reciprocal rank fusion.
    Rrf,
    /// `DBSF` — distribution-based score fusion.
    Dbsf,
}

/// Target collection of a query statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryCollection {
    /// Collection named after `FROM`.
    Explicit(String),
    /// CTE without its own `FROM`; inherits the enclosing query's collection.
    Inherited,
}

/// Where a prefetch stage draws its candidates from.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PrefetchSource {
    /// Reference to a named `WITH` CTE.
    Cte(String),
    /// Inline `QUERY` sub-statement.
    Query(Box<QueryStmt>),
}

/// `LOOKUP FROM <collection> [VECTOR <name>]` group-value join hint.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LookupSpec {
    /// Collection to read group values from.
    pub collection: String,
    /// Optional named vector used by the lookup.
    pub vector: Option<String>,
}

/// One stage of the `PREFETCH (…)` pipeline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Prefetch {
    /// CTE reference or inline query.
    pub source: PrefetchSource,
    /// Prefetch-level `WHERE` override.
    pub filter: Option<Box<FilterExpr>>,
    /// Prefetch-level `SCORE THRESHOLD` override.
    pub score_threshold: Option<f64>,
    /// Optional cross-collection lookup for this stage.
    pub lookup: Option<LookupSpec>,
}

/// `QUERY` expression body — the retrieval strategy and its inputs.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryExpr {
    /// `QUERY POINTS (ids)` — direct retrieval of the listed points.
    Points {
        /// Point IDs to fetch.
        ids: Vec<PointId>,
    },
    /// `QUERY [NEAREST] <input> FROM <coll>` — vector nearest-neighbor search.
    Nearest {
        /// Embeddable input, vector, or reference point.
        input: QueryInput,
        /// `USING` vector target; schema-resolved when `None`.
        using: Option<VectorTarget>,
        /// Multi-stage `PREFETCH` pipeline.
        prefetch: Vec<Prefetch>,
        /// MMR re-diversification settings.
        mmr: Option<Box<MmrConfig>>,
    },
    /// `QUERY RECOMMEND POSITIVE … [NEGATIVE …]` — recommend from examples.
    Recommend {
        /// Examples to move toward.
        positive: Vec<QueryInput>,
        /// Examples to move away from.
        negative: Vec<QueryInput>,
        /// Scoring strategy; server default when `None`.
        strategy: Option<RecommendStrategy>,
        /// `USING` vector target; schema-resolved when `None`.
        using: Option<VectorTarget>,
        /// Multi-stage `PREFETCH` pipeline.
        prefetch: Vec<Prefetch>,
    },
    /// `QUERY CONTEXT (POSITIVE … NEGATIVE …)` — search guided by example pairs.
    Context {
        /// Positive/negative example pairs.
        pairs: Vec<ContextPair>,
        /// `USING` vector target; schema-resolved when `None`.
        using: Option<VectorTarget>,
        /// Multi-stage `PREFETCH` pipeline.
        prefetch: Vec<Prefetch>,
    },
    /// `QUERY DISCOVER TARGET … CONTEXT (…)` — discovery from target plus pairs.
    Discover {
        /// Primary target input.
        target: QueryInput,
        /// Guiding positive/negative pairs.
        context: Vec<ContextPair>,
        /// `USING` vector target; schema-resolved when `None`.
        using: Option<VectorTarget>,
        /// Multi-stage `PREFETCH` pipeline.
        prefetch: Vec<Prefetch>,
    },
    /// `QUERY ORDER BY field [ASC|DESC]` — payload-value ordering.
    OrderBy {
        /// Payload field to sort on.
        field: String,
        /// Sort direction (`ASC` default).
        direction: OrderDirection,
    },
    /// `QUERY SAMPLE RANDOM` — random sample of points.
    SampleRandom,
    /// `QUERY FUSION RRF|DBSF` — fuse results of the prefetch stages.
    Fusion {
        /// Fusion algorithm.
        method: FusionMethod,
        /// Stages whose results are fused (must be non-empty).
        prefetch: Vec<Prefetch>,
    },
    /// `QUERY FORMULA <expr> [DEFAULTS (…)]` — formula-expression rescoring.
    Formula {
        /// Rescoring expression tree.
        expression: Box<FormulaExpr>,
        /// `DEFAULTS` bindings for formula variables.
        defaults: Vec<(String, Value)>,
        /// Candidate stages the formula rescoring applies to.
        prefetch: Vec<Prefetch>,
    },
    /// `QUERY RELEVANCE FEEDBACK TARGET … FEEDBACK (…)` — naive feedback search.
    RelevanceFeedback {
        /// Base target input.
        target: QueryInput,
        /// Weighted feedback examples.
        feedback: Vec<FeedbackItem>,
        /// `STRATEGY NAIVE (a = …, b = …, c = …)` weights.
        strategy: FeedbackStrategy,
        /// `USING` vector target; schema-resolved when `None`.
        using: Option<VectorTarget>,
        /// Multi-stage `PREFETCH` pipeline.
        prefetch: Vec<Prefetch>,
    },
    /// `QUERY HYBRID TEXT … [DENSE n] [SPARSE n] [FUSION m]` — dense+sparse fusion.
    Hybrid {
        /// Query text embedded for both stages.
        text: String,
        /// Optional embedding model override.
        model: Option<String>,
        /// Named dense vector; schema-resolved when `None`.
        dense_vector: Option<String>,
        /// Named sparse vector; schema-resolved when `None`.
        sparse_vector: Option<String>,
        /// Fusion method for the two stages.
        fusion: FusionMethod,
    },
    /// `QUERY RERANK <input> MODEL '…'` — late-interaction rerank over prefetch.
    Rerank {
        /// Query input embedded via `USING`.
        input: QueryInput,
        /// ColBERT-style late-interaction model.
        model: String,
        /// Dense (or multivector) target for document embeddings.
        using: Option<VectorTarget>,
        /// Multi-stage `PREFETCH` pipeline.
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
        /// Candidate stages whose documents are reranked.
        prefetch: Vec<Prefetch>,
    },
}

/// `PARAMS (quantization = {…})` overrides for quantized index search.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantizationSearchParams {
    /// Ignore quantized data and search the original vectors.
    pub ignore: Option<bool>,
    /// Rescore candidates with original vectors when available.
    pub rescore: Option<bool>,
    /// Oversampling factor for candidate retrieval.
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

/// `PARAMS (…)` execution knobs for a query.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchParams {
    /// HNSW candidate list size (`hnsw_ef`).
    pub hnsw_ef: Option<u64>,
    /// Force exact (brute-force) search.
    pub exact: Option<bool>,
    /// Enable or disable ACORN filter-aware search.
    pub acorn: Option<bool>,
    /// ACORN selectivity ceiling in (0, 1]. Only valid with `acorn = true`.
    pub max_selectivity: Option<f64>,
    /// Restrict the search to indexed points only.
    pub indexed_only: Option<bool>,
    /// Quantization search overrides.
    pub quantization: Option<QuantizationSearchParams>,
    /// RRF smoothing constant `k`.
    pub rrf_k: Option<u64>,
    /// Per-prefetch RRF weights.
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

/// `WITH PAYLOAD` payload projection selector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PayloadSelector {
    /// `true` — return all payload fields.
    All,
    /// `false` — return no payload.
    None,
    /// `INCLUDE (fields)` — only the listed fields.
    Include(Vec<String>),
    /// `EXCLUDE (fields)` — everything except the listed fields.
    Exclude(Vec<String>),
}

/// `WITH VECTOR` vector projection selector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorSelector {
    /// `true` — return all vectors.
    All,
    /// `false` — return no vectors.
    None,
    /// Return only the named vectors.
    Names(Vec<String>),
}

/// `WITH PAYLOAD` / `WITH VECTOR` result projection of a query.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QueryOutput {
    /// Payload selector; `None` defaults to all payload fields.
    pub payload: Option<PayloadSelector>,
    /// Vector selector; `None` returns no vectors.
    pub vectors: Option<VectorSelector>,
}

/// `GROUP BY field [SIZE n] [LOOKUP FROM c [VECTOR v]]` settings.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupSpec {
    /// Payload field used as the group key.
    pub field: String,
    /// Maximum hits per group.
    pub size: Option<u64>,
    /// Optional collection used to resolve group values.
    pub lookup: Option<String>,
}

/// `LIMIT` / `OFFSET` result paging.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PageSpec {
    /// Maximum number of results (or groups).
    pub limit: Option<u64>,
    /// Number of results (or groups) to skip.
    pub offset: Option<u64>,
}

/// One named common table expression: `name AS (QUERY …)`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cte {
    /// CTE name, referenced case-insensitively by prefetches.
    pub name: String,
    /// The CTE's query body.
    pub query: Box<QueryStmt>,
}

/// A full `QUERY` statement: CTEs, expression, clauses, and output options.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QueryStmt {
    /// Leading `WITH` definitions.
    pub ctes: Vec<Cte>,
    /// Target collection (explicit or inherited).
    pub collection: QueryCollection,
    /// Retrieval strategy body.
    pub expression: QueryExpr,
    /// `WHERE` filter.
    pub filter: Option<Box<FilterExpr>>,
    /// `PARAMS (…)` execution settings.
    pub params: Option<SearchParams>,
    /// `SCORE THRESHOLD` minimum score.
    pub score_threshold: Option<f64>,
    /// `GROUP BY` settings.
    pub group: Option<GroupSpec>,
    /// `WITH PAYLOAD` / `WITH VECTOR` projection.
    pub output: QueryOutput,
    /// `LIMIT` / `OFFSET` paging.
    pub page: PageSpec,
    /// `SHARD '<key>'` routing for tenant-partitioned collections.
    pub shard_key: Option<String>,
}

/// `SCROLL FROM <collection> … LIMIT n` — cursor-based point iteration.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScrollStmt {
    /// Collection to scroll.
    pub collection: String,
    /// Maximum number of points per page.
    pub limit: u64,
    /// Optional `WHERE` filter.
    pub filter: Option<Box<FilterExpr>>,
    /// `AFTER` cursor — resume scrolling after this point ID.
    pub after: Option<PointId>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
    /// Optional `WITH VECTOR` selector. Defaults to no vectors when `None`.
    pub with_vector: Option<VectorSelector>,
}

/// Role of an `EMBED` directive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbedKind {
    /// Dense embedding (the default role).
    Dense {
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Sparse (e.g. BM25) embedding.
    Sparse {
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Multivector / ColBERT bag (`embed_multi` → MultiDense).
    Multi {
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Image / CLIP vision path or URL → dense vector (`embed_image`).
    Image {
        /// Optional embedding model override.
        model: Option<String>,
    },
}

/// `EMBED <field> INTO <vector> [USING …]` directive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EmbedDirective {
    /// Payload field providing the embedding input.
    pub source_field: String,
    /// Named vector to write into.
    pub target_vector: String,
    /// Embedding role and model for this directive.
    pub kind: EmbedKind,
}

/// Upsert-level `USING` embedding clause.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbeddingSpec {
    /// `USING DENSE` / `USING MODEL` / `USING VECTOR` — dense embedding.
    Dense {
        /// Optional embedding model.
        model: Option<String>,
        /// Optional target vector name.
        vector: Option<String>,
        /// Optional payload field to embed.
        field: Option<String>,
    },
    /// `USING SPARSE` — sparse embedding.
    Sparse {
        /// Optional sparse model (e.g. BM25-compatible).
        model: Option<String>,
        /// Optional target vector name.
        vector: Option<String>,
        /// Optional payload field to embed.
        field: Option<String>,
    },
    /// `USING HYBRID` — parallel dense + sparse embedding.
    Hybrid {
        /// Optional dense embedding model.
        dense_model: Option<String>,
        /// Optional dense target vector name.
        dense_vector: Option<String>,
        /// Optional dense input payload field.
        dense_field: Option<String>,
        /// Optional sparse embedding model.
        sparse_model: Option<String>,
        /// Optional sparse target vector name.
        sparse_vector: Option<String>,
        /// Optional sparse input payload field.
        sparse_field: Option<String>,
    },
    /// Multivector / ColBERT: text → bag of token vectors for a named multi slot.
    MultiVector {
        /// Optional multivector embedding model.
        model: Option<String>,
        /// Optional target multivector name.
        vector: Option<String>,
        /// Optional payload field to embed.
        field: Option<String>,
    },
    /// Image / CLIP vision: payload field holds a path or URL → dense vector.
    Image {
        /// Optional CLIP vision model.
        model: Option<String>,
        /// Optional target dense vector name.
        vector: Option<String>,
        /// Optional payload field holding the image path or URL.
        field: Option<String>,
    },
    /// Combined specs (e.g. DENSE + SPARSE + MULTI VECTOR colbert).
    Multi(Vec<EmbeddingSpec>),
}

/// One `VALUES {…}` object of an `UPSERT INTO`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpsertPoint {
    /// Point identifier (unsigned integer or string).
    pub id: PointId,
    /// Optional pre-computed vectors, unnamed or by name.
    pub vectors: Option<PointVectors>,
    /// Remaining object entries as payload key-value pairs.
    pub payload: Vec<(String, Value)>,
}

/// `UPSERT INTO <collection> VALUES …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpsertStmt {
    /// Target collection.
    pub collection: String,
    /// Points to upsert.
    pub points: Vec<UpsertPoint>,
    /// Optional `USING` embedding clause.
    pub embedding: Option<EmbeddingSpec>,
    /// `EMBED <field> INTO <vector>` directives.
    pub embed: Vec<EmbedDirective>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
}

/// Distance metric of a `VECTOR(size, distance)` definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorDistance {
    /// `COSINE` — cosine similarity distance.
    Cosine,
    /// `DOT` — dot product distance.
    Dot,
    /// `EUCLID` — Euclidean distance.
    Euclid,
    /// `MANHATTAN` — Manhattan (L1) distance.
    Manhattan,
}

/// Comparator for multivector (late-interaction) scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MultivectorComparator {
    /// `max_sim` — maximum pairwise vector similarity.
    MaxSim,
}

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

/// `WITH QUANTIZATION (type = …)` quantization family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QuantizationType {
    /// `scalar` — scalar (int8-style) quantization.
    Scalar,
    /// `binary` — binary quantization.
    Binary,
    /// `product` — product quantization.
    Product,
    /// `turbo` — Turbo quantization (`bits` 1, 1.5, 2, or 4).
    Turbo,
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

/// `CLEAR PAYLOAD FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClearPayloadStmt {
    /// Target collection.
    pub collection: String,
    /// Points whose payload is cleared.
    pub selector: PointSelector,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
}

/// `DELETE VECTOR <names> FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteVectorStmt {
    /// Target collection.
    pub collection: String,
    /// Points whose named vectors are removed.
    pub selector: PointSelector,
    /// Named vectors to remove.
    pub vector_names: Vec<String>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
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

/// `COUNT FROM <collection> [WHERE …]` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CountStmt {
    /// Collection to count (explicit or inherited).
    pub collection: QueryCollection,
    /// Optional `WHERE` filter.
    pub filter: Option<Box<FilterExpr>>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
    /// `WITH (exact = …)` — require exact counts.
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

/// Point selection used by mutation statements.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointSelector {
    /// A single point ID.
    Id(PointId),
    /// An explicit list of point IDs.
    Ids(Vec<PointId>),
    /// All points matching a filter.
    Filter(Box<FilterExpr>),
}

/// `DELETE FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteStmt {
    /// Target collection.
    pub collection: String,
    /// Points to delete.
    pub selector: PointSelector,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
}

/// `UPDATE <collection> SET VECTOR … WHERE id = …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdateVectorStmt {
    /// Target collection.
    pub collection: String,
    /// Point whose vector is replaced.
    pub point_id: PointId,
    /// New vector value.
    pub vector: VectorValue,
    /// Named vector to update; `None` targets the unnamed vector.
    pub vector_name: Option<String>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
}

/// `DELETE PAYLOAD <keys> FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeletePayloadStmt {
    /// Target collection.
    pub collection: String,
    /// Payload keys to remove.
    pub keys: Vec<String>,
    /// Points whose payload keys are removed.
    pub selector: PointSelector,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
}

/// `UPDATE <collection> SET PAYLOAD = {…} WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdatePayloadStmt {
    /// Target collection.
    pub collection: String,
    /// Points to update.
    pub selector: PointSelector,
    /// Payload keys to merge into the points.
    pub payload: Vec<(String, Value)>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<String>,
}

/// Top-level QQL statement parsed from a script.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `QUERY …` retrieval (all `QueryExpr` forms).
    Query(Box<QueryStmt>),
    /// `SCROLL …` cursor iteration.
    Scroll(Box<ScrollStmt>),
    /// `UPSERT INTO …` point write.
    Upsert(Box<UpsertStmt>),
    /// `CREATE COLLECTION …` DDL.
    CreateCollection(Box<CreateCollectionStmt>),
    /// `CREATE INDEX …` DDL.
    CreateIndex(Box<CreateIndexStmt>),
    /// `DROP INDEX …` DDL.
    DropIndex(Box<DropIndexStmt>),
    /// `CREATE SHARD KEY …` DDL.
    CreateShardKey(Box<CreateShardKeyStmt>),
    /// `DROP SHARD KEY …` DDL.
    DropShardKey(Box<DropShardKeyStmt>),
    /// `ALTER COLLECTION …` DDL.
    AlterCollection(Box<AlterCollectionStmt>),
    /// `DROP COLLECTION …` DDL.
    DropCollection(Box<DropCollectionStmt>),
    /// `SHOW COLLECTIONS` listing.
    ShowCollections,
    /// `SHOW COLLECTION <name>` detail.
    ShowCollection(String),
    /// `SHOW SHARD KEYS ON COLLECTION <name>` listing.
    ShowShardKeys(String),
    /// `DELETE FROM …` point removal.
    Delete(Box<DeleteStmt>),
    /// `CLEAR PAYLOAD …` payload wipe.
    ClearPayload(Box<ClearPayloadStmt>),
    /// `DELETE PAYLOAD <keys> …` payload key removal.
    DeletePayload(Box<DeletePayloadStmt>),
    /// `DELETE VECTOR <names> …` named vector removal.
    DeleteVector(Box<DeleteVectorStmt>),
    /// `UPDATE … SET VECTOR …` vector replacement.
    UpdateVector(Box<UpdateVectorStmt>),
    /// `UPDATE … SET PAYLOAD …` payload merge.
    UpdatePayload(Box<UpdatePayloadStmt>),
    /// `COUNT …` point counting.
    Count(Box<CountStmt>),
    /// `FACET …` categorical aggregation.
    Facet(Box<FacetStmt>),
    /// `SHOW QUOTAS` listing.
    ShowQuotas,
    /// `SET QUOTA (…)` cluster quota replacement.
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
