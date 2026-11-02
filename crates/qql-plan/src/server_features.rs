//! Server-version requirements of a planned operation.
//!
//! QQL 1.9 lowers some statements to wire shapes that only Qdrant >= 1.20
//! understands (server-side BM25 text queries, text index `scoring`, the
//! `code` tokenizer). Backends use [`PlannedOperation::qdrant_1_20_feature`]
//! to turn an older server's opaque decode error into a typed QQL error.

use crate::plan::PlannedOperation;
use crate::types::{PrefetchRequest, QueryRequest, QueryVariant, TextTokenizer};

/// Minimum Qdrant version for the QQL 1.9 server-side text features.
pub const QDRANT_TEXT_BM25_MIN_VERSION: &str = "1.20";

impl PlannedOperation {
    /// The first Qdrant >= 1.20-only feature this operation uses, if any
    /// (`"QUERY BM25"`, `"text index scoring = 'bm25'"`, `"tokenizer = 'code'"`).
    pub fn qdrant_1_20_feature(&self) -> Option<&'static str> {
        match self {
            PlannedOperation::Query { request, .. } => query_request_feature(request),
            PlannedOperation::QueryGroups { request, .. } => {
                variant_feature(&request.query).or_else(|| prefetches_feature(&request.prefetch))
            }
            PlannedOperation::CrossRerank { candidates, .. } => candidates
                .iter()
                .find_map(|(_, request)| query_request_feature(request)),
            PlannedOperation::CreateIndex { request, .. } => {
                if request.options.scoring.is_some() {
                    Some("text index scoring = 'bm25'")
                } else if request.options.tokenizer == Some(TextTokenizer::Code) {
                    Some("text index tokenizer = 'code'")
                } else {
                    None
                }
            }
            PlannedOperation::Batch { operations, .. } => operations
                .iter()
                .find_map(PlannedOperation::qdrant_1_20_feature),
            _ => None,
        }
    }
}

/// [`PlannedOperation::qdrant_1_20_feature`] for one query request (also used
/// for grouped query batches).
pub fn query_request_feature(request: &QueryRequest) -> Option<&'static str> {
    variant_feature(&request.query).or_else(|| prefetches_feature(&request.prefetch))
}

fn variant_feature(variant: &QueryVariant) -> Option<&'static str> {
    matches!(variant, QueryVariant::Text(_)).then_some("QUERY BM25")
}

fn prefetches_feature(prefetch: &[PrefetchRequest]) -> Option<&'static str> {
    prefetch.iter().find_map(|stage| {
        stage
            .query
            .as_ref()
            .and_then(variant_feature)
            .or_else(|| stage.prefetch.as_deref().and_then(prefetches_feature))
    })
}
