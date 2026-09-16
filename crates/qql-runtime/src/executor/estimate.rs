//! Corpus-true BM25 `avg_len` estimation from sampled payloads.
//!
//! Qdrant computes `avg_len` nowhere: it is a user-supplied parameter, and
//! the `256` default assumes document-length text. Pointing a title/tag
//! vector at `avg_len = 256` silently skews every weight. This module closes
//! the loop with real data: scroll a sample of the field's actual texts,
//! measure them with the same pipeline that will embed the writes, and feed
//! the mean back into `bm25_avg_len` (config or embedder) before ingesting.

use qql_core::error::QqlError;
use qql_embed::{AvgLenEstimate, estimate_avg_len};

use super::{Executor, OnError};

impl Executor {
    /// Estimate a corpus-true `avg_len` for `field` from up to `sample`
    /// points of `collection`.
    ///
    /// Scrolls the collection, extracts the string at `field` (a top-level
    /// payload key matched ASCII-case-insensitively, exactly like the
    /// writer's field lookup), and returns the mean post-pipeline token count measured with this executor's BM25 pipeline
    /// (embedder's [`qql_embed::Embedder::bm25_text_config`], else the runtime
    /// [`crate::config::QqlConfig`], else Qdrant defaults) — exactly the
    /// `doc_len` the TF formula consumes, so the estimate is self-consistent
    /// by construction.
    ///
    /// Returns `None` when the sample holds no usable texts (empty
    /// collection, missing field, or only empty strings): an empty corpus
    /// has no meaningful average, so keeping the default beats storing
    /// `avg_len = 0` (which validation rejects). Feed
    /// `estimate.mean` back into `bm25_avg_len` and re-ingest to apply;
    /// vectors written before the change keep their old weights.
    ///
    /// Pipelines the core cannot execute (notably `multilingual`, which
    /// needs a segmentation stack the lean core does not ship) fail here
    /// with `QQL-VALIDATION-CONFIG` — including on edge backends whose
    /// engine would embed them fine.
    pub async fn estimate_bm25_avg_len(
        &self,
        collection: &str,
        field: &str,
        sample: usize,
    ) -> Result<Option<AvgLenEstimate>, QqlError> {
        if collection.trim().is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-COLLECTION",
                "estimate_bm25_avg_len needs a non-empty collection name",
                None,
            ));
        }
        if field.trim().is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-FIELD",
                "estimate_bm25_avg_len needs a non-empty payload field",
                None,
            ));
        }
        if sample == 0 {
            return Ok(None);
        }
        // Single-quoted with SQL `''` escaping: collection names are parser
        // identifiers, and this is the only quoting the grammar accepts.
        let quoted = collection.replace('\'', "''");
        let report = self
            .execute(
                &format!("SCROLL FROM '{quoted}' LIMIT {sample}"),
                OnError::Stop,
            )
            .await?;
        let Some(response) = report.results.into_iter().next() else {
            return Ok(None);
        };
        let Some(hits) = response.hits_ref() else {
            return Ok(None);
        };
        let pipeline = match self.embedder.as_ref() {
            Some(embedder) => embedder.bm25_text_config().pipeline(),
            None => match self.config.as_ref() {
                Some(config) => config.bm25_text_config()?.pipeline(),
                None => qql_embed::Bm25TextConfig::default().pipeline(),
            },
        };
        let mut texts = Vec::new();
        for hit in hits {
            // Same field rule as the writer (`collect_text_targets`): a
            // top-level key matched ASCII-case-insensitively, holding a
            // non-empty string. Arrays, nested objects, dotted paths, and
            // empty strings are never embedded, so they must not dilute the
            // mean either — otherwise the estimate would describe documents
            // the formula never sees.
            if let Some(payload) = hit.payload.as_ref()
                && let Some((_, serde_json::Value::String(text))) = payload
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(field))
                && !text.is_empty()
            {
                texts.push(text.clone());
            }
        }
        estimate_avg_len(texts.iter().map(String::as_str), &pipeline)
    }
}
