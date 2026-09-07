//! Typed AST for QUERY statements and expressions.

use super::types::*;
use crate::ast::{FilterExpr, FormulaExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

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
        /// Parameter placeholder (`:name` or `?idx`) when the text was not provided as a literal string.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        text_param: Option<String>,
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
    /// Parameter placeholder (`:name`) for target query input.
    Param(
        String,
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        Option<crate::error::Span>,
    ),
    /// Positional parameter placeholder (`?`) for target query input.
    PositionalParam(
        usize,
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        Option<crate::error::Span>,
    ),
}

impl QueryInput {
    /// Construct an unlocated named parameter placeholder.
    pub fn param(name: impl Into<String>) -> Self {
        Self::Param(name.into(), None)
    }

    /// Construct a located named parameter placeholder.
    pub fn param_with_span(name: impl Into<String>, span: crate::error::Span) -> Self {
        Self::Param(name.into(), Some(span))
    }

    /// Construct an unlocated positional parameter placeholder.
    pub fn positional_param(idx: usize) -> Self {
        Self::PositionalParam(idx, None)
    }

    /// Construct a located positional parameter placeholder.
    pub fn positional_param_with_span(idx: usize, span: crate::error::Span) -> Self {
        Self::PositionalParam(idx, Some(span))
    }

    /// Extract the parameter source span if present.
    pub fn param_span(&self) -> Option<crate::error::Span> {
        match self {
            Self::Param(_, span) | Self::PositionalParam(_, span) => *span,
            _ => None,
        }
    }
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
        /// Parameter placeholder (`:name` or `?idx`) when the text was not provided as a literal string.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        text_param: Option<String>,
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
        /// Parameter placeholder (`:name` or `?idx`) when the query was not provided as a literal string.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        query_param: Option<String>,
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
    /// Parameter name for limit (e.g. `:lim`), if unbound.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub limit_param: Option<String>,
    /// Parameter name for offset (e.g. `:off`), if unbound.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub offset_param: Option<String>,
    /// Source span of the limit parameter placeholder, if unbound.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub limit_span: Option<crate::error::Span>,
    /// Source span of the offset parameter placeholder, if unbound.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub offset_span: Option<crate::error::Span>,
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
