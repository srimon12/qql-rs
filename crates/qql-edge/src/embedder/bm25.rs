//! Local BM25 helpers: engine config mapping, sparse fastembed batch runner.
//!
//! Pure move from `embedder.rs` (size hygiene split).

use qdrant_edge::bm25_embed::EdgeBm25Config;
use qql_core::error::QqlError;
use qql_embed::{Bm25TextConfig, SparseVector};

use super::FastEmbedder;
use super::err;

/// Convert an edge BM25 sparse vector into the transport-neutral type.
pub(crate) fn to_qql_sparse(sv: qdrant_edge::SparseVector) -> SparseVector {
    SparseVector {
        indices: sv.indices,
        values: sv.values,
    }
}

/// Map a validated [`Bm25TextConfig`] onto qdrant-edge's [`EdgeBm25Config`].
///
/// `k`/`b`/`avg_len`, `tokenizer`, `language`, `lowercase`, `ascii_folding`,
/// and token length limits forward one-to-one (same names, same defaults).
/// Explicit stemmer/stopword overrides forward through a serde JSON
/// round-trip (`EdgeBm25Config` derives both directions, so the unexported
/// `StemmingAlgorithm`/`StopwordsInterface` types never need naming): the
/// JSON shapes mirror Qdrant's REST `Bm25Config`, pinned by
/// `edge_bm25_overrides_round_trip` below. `Multilingual` forwards to the
/// real engine, which ships the segmentation stack (unlike the lean core
/// pipeline).
///
/// `Bm25Params::new` already rejects NaN/±Inf, so the `NotNan` conversions
/// are infallible in practice; they are mapped instead of unwrapped so an
/// impossible failure would still surface as a typed error.
pub(crate) fn edge_bm25_config(text: &Bm25TextConfig) -> Result<EdgeBm25Config, QqlError> {
    use ordered_float::NotNan;
    use qdrant_edge::TokenizerType;
    let params = &text.params;
    let k = NotNan::new(params.k1()).map_err(|e| err(format!("invalid bm25 k1: {e}")))?;
    let b = NotNan::new(params.b()).map_err(|e| err(format!("invalid bm25 b: {e}")))?;
    let avg_len =
        NotNan::new(params.avg_len()).map_err(|e| err(format!("invalid bm25 avg_len: {e}")))?;
    let tokenizer = match text.tokenizer {
        qql_embed::Tokenizer::Word => TokenizerType::Word,
        qql_embed::Tokenizer::Whitespace => TokenizerType::Whitespace,
        qql_embed::Tokenizer::Prefix => TokenizerType::Prefix,
        qql_embed::Tokenizer::Multilingual => TokenizerType::Multilingual,
    };
    let mut cfg = EdgeBm25Config {
        k,
        b,
        avg_len,
        tokenizer,
        language: Some(text.language.name().to_string()),
        lowercase: Some(text.lowercase),
        ascii_folding: Some(text.ascii_folding),
        min_token_len: text.min_token_len,
        max_token_len: text.max_token_len,
        ..Default::default()
    };
    if text.stopwords.is_some() || text.stemmer.is_some() {
        cfg = merge_text_overrides(cfg, text)?;
    }
    Ok(cfg)
}

/// Merge explicit stopword/stemmer overrides into an [`EdgeBm25Config`]
/// through its serde JSON form (see [`edge_bm25_config`]).
///
/// Shapes mirror Qdrant's REST `Bm25Config`: stopwords become
/// `{"languages": [...], "custom": [...]}` (an empty selection disables
/// filtering, exactly like Qdrant's empty `Set`); stemmers become
/// `{"type": "snowball", "language": "<name>"}` or `{"type": "none"}`.
/// A shape mismatch (third-party drift) fails closed instead of silently
/// dropping the overrides.
fn merge_text_overrides(
    cfg: EdgeBm25Config,
    text: &Bm25TextConfig,
) -> Result<EdgeBm25Config, QqlError> {
    let mut json = serde_json::to_value(&cfg)
        .map_err(|e| err(format!("edge BM25 config serialize failed: {e}")))?;
    if let Some(selection) = &text.stopwords {
        let languages: Vec<&str> = selection
            .languages
            .iter()
            .map(|language| language.name())
            .collect();
        json["stopwords"] = serde_json::json!({
            "languages": languages,
            "custom": selection.custom,
        });
    }
    if let Some(stemmer) = &text.stemmer {
        json["stemmer"] = match stemmer {
            qql_embed::Stemmer::Snowball(language) => {
                if language.stem_algorithm().is_none() {
                    // Directly constructed (parse rejects these): the engine
                    // has no such stemmer either, so fail here with the cause.
                    return Err(err(format!(
                        "edge BM25 stemmer unavailable for language {:?}",
                        language.name()
                    )));
                }
                serde_json::json!({"type": "snowball", "language": language.name()})
            }
            qql_embed::Stemmer::Armenian => {
                serde_json::json!({"type": "snowball", "language": "armenian"})
            }
            qql_embed::Stemmer::Tamil => {
                serde_json::json!({"type": "snowball", "language": "tamil"})
            }
            qql_embed::Stemmer::Disabled => serde_json::json!({"type": "none"}),
        };
    }
    serde_json::from_value(json).map_err(|e| err(format!("edge BM25 override shape mismatch: {e}")))
}

/// Validate `model` against the embedder's sparse configuration: with a
/// fastembed sparse model locked in, only that model (or default/empty) is
/// allowed; without one, only default/empty is allowed (local wire-compatible
/// BM25 handles it).
pub(crate) fn ensure_sparse_model_allowed(
    embedder: &FastEmbedder,
    model: &str,
) -> Result<(), QqlError> {
    let Some(ref sparse) = embedder.sparse else {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        return Ok(());
    };
    if !(embedder.accepts_sparse_model(model)
        || model.is_empty()
        || model.eq_ignore_ascii_case("default"))
    {
        return Err(err(format!(
            "local sparse embedder is locked to '{}' ({}); cannot satisfy MODEL '{model}'",
            sparse.model_name, sparse.model_code
        )));
    }
    Ok(())
}

/// Run the configured fastembed sparse model over a batch of texts.
pub(crate) async fn embed_sparse_fastembed_batch(
    embedder: &FastEmbedder,
    texts: Vec<String>,
) -> Result<Vec<SparseVector>, QqlError> {
    let Some(ref sparse) = embedder.sparse else {
        return Err(err("no fastembed sparse model configured"));
    };
    let model_arc = sparse.model.clone();

    let embeddings = tokio::task::spawn_blocking(move || {
        let mut model = model_arc
            .lock()
            .map_err(|e| err(format!("fastembed sparse mutex poisoned: {e}")))?;
        model
            .embed(texts, None)
            .map_err(|e| err(format!("fastembed SparseTextEmbedding failed: {e}")))
    })
    .await
    .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

    Ok(embeddings
        .into_iter()
        .map(|e| SparseVector {
            indices: e.indices.iter().map(|&i| i as u32).collect(),
            values: e.values.clone(),
        })
        .collect())
}
