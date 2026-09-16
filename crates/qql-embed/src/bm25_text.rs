//! Full BM25 text-processing pipeline, option-compatible with Qdrant.
//!
//! Qdrant's `Bm25Config` (REST) / `EdgeBm25Config` (edge) exposes, per sparse
//! vector: `k`, `b`, `avg_len`, `tokenizer`, `language`, `lowercase`,
//! `ascii_folding`, `stopwords`, `stemmer`, `min_token_len`, `max_token_len`.
//! [`Bm25TextConfig`] mirrors that surface with the same defaults (word
//! tokenizer, English, lowercase on, folding off, language stopwords/stemmer,
//! no length limits), and [`Bm25Pipeline`] executes it in Qdrant's exact
//! stage order: fold → lowercase → stopwords → stem → length check.
//!
//! Wire compatibility notes (all verified against Qdrant's implementation):
//! - Token IDs are Qdrant's `token_id` (murmur3-32 seed 0, `unsigned_abs`).
//! - The TF formula uses the same fused operation order as `lib/bm25`.
//! - `k1 = 0` is accepted (binary weighting), like Qdrant's validator.
//! - Stopword lists are ported verbatim from Qdrant's segment crate.
//! - Folding uses Qdrant's Lucene-derived mapping.
//! - `Multilingual` needs the `charabia` tokenizer and fails closed without
//!   it; Japanese falls back to generic segmentation (Qdrant uses a
//!   `vaporetto` model file we do not ship) — both are explicit errors, never
//!   silent degradation.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::LazyLock;

use qql_core::error::QqlError;
use rust_stemmers::Stemmer as SnowballStemmer;

use super::bm25_fold::fold_to_ascii_cow;
use super::bm25_lang::Language;
use super::bm25_stopwords::stopwords_for;
use super::sparse::{Bm25Params, SparseVector, token_id};

fn config_error(message: String) -> QqlError {
    QqlError::validation("QQL-VALIDATION-CONFIG", message, None)
}

/// Tokenizer, mirroring Qdrant's `TokenizerType` names.
///
/// `Multilingual` parses but fails closed at build time: it needs the
/// `charabia`/`vaporetto` segmentation stack, which `qql-embed` deliberately
/// does not depend on (lean core for WASM/edge; no model files to ship).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tokenizer {
    /// Split on non-alphanumeric boundaries (Qdrant default).
    #[default]
    Word,
    /// Split on Unicode whitespace only.
    Whitespace,
    /// Document side expands all n-grams; query side keeps the longest only.
    Prefix,
    /// Script-aware segmentation (requires tokenizer support not compiled in).
    Multilingual,
}

impl Tokenizer {
    /// Parse a Qdrant `tokenizer` name (`"word"`, `"whitespace"`, `"prefix"`,
    /// `"multilingual"`), ASCII-case-insensitively. Anything else fails closed.
    pub fn parse(name: &str) -> Result<Self, QqlError> {
        match name.to_ascii_lowercase().as_str() {
            "word" => Ok(Self::Word),
            "whitespace" => Ok(Self::Whitespace),
            "prefix" => Ok(Self::Prefix),
            "multilingual" => Ok(Self::Multilingual),
            _ => Err(config_error(format!(
                "unsupported bm25 tokenizer: {name:?}"
            ))),
        }
    }

    /// Canonical Qdrant spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Whitespace => "whitespace",
            Self::Prefix => "prefix",
            Self::Multilingual => "multilingual",
        }
    }
}

/// Stemmer selection, mirroring Qdrant's `Option<StemmingAlgorithm>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stemmer {
    /// Snowball stemmer for an explicit language (Qdrant's `Snowball`).
    /// Only the 17 Snowball languages are reachable; the rest fail closed at
    /// parse time, exactly like Qdrant's edge builder.
    Snowball(Language),
    /// Explicitly no stemming (Qdrant's `{"type": "none"}`). Differs from
    /// leaving `stemmer` unset, which falls back to the language default.
    Disabled,
}

impl Stemmer {
    /// Parse `"none"` (disable) or a language name/alias. Anything else —
    /// including languages without a Snowball stemmer — fails closed.
    pub fn parse(name: &str) -> Result<Self, QqlError> {
        if name.eq_ignore_ascii_case("none") {
            return Ok(Self::Disabled);
        }
        let language = Language::parse(name)?;
        if language.stem_algorithm().is_none() {
            return Err(config_error(format!(
                "bm25 stemmer unavailable for language {:?}: no Snowball stemmer (use \"none\" to disable)",
                language.name()
            )));
        }
        Ok(Self::Snowball(language))
    }
}

/// Stopword selection, mirroring Qdrant's `StopwordsInterface`.
///
/// `None` (the `stopwords` field left unset) keeps the processing
/// language's list. `Some` **replaces** it: an empty [`Stopwords`] disables
/// filtering entirely (Qdrant's `Set` default), otherwise the listed
/// languages plus custom words are merged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stopwords {
    /// Additional language lists to merge.
    pub languages: Vec<Language>,
    /// Custom words to merge (compared post-normalization, like Qdrant,
    /// which lowercases entries when `lowercase` is on).
    pub custom: Vec<String>,
}

/// Full BM25 text-processing configuration — mirrors Qdrant's `Bm25Config`.
///
/// Construct via [`Bm25TextConfig::resolve`] (flat stringly options, the
/// single choke point every config surface calls) or struct-literal with
/// `..Default::default()`. Numeric parameters validate through
/// [`Bm25Params`]; names parse through [`Language`]/[`Tokenizer`]/[`Stemmer`].
#[derive(Debug, Clone, PartialEq)]
pub struct Bm25TextConfig {
    /// Validated `k1`/`b`/`avg_len`.
    pub params: Bm25Params,
    /// Tokenizer (default [`Tokenizer::Word`]).
    pub tokenizer: Tokenizer,
    /// Language for default stopwords/stemmer (default English, like Qdrant).
    pub language: Language,
    /// Lowercase before matching (default true, like Qdrant).
    pub lowercase: bool,
    /// Lucene ASCII folding before lowercasing (default false, like Qdrant).
    pub ascii_folding: bool,
    /// Stopword override (`None` = language default).
    pub stopwords: Option<Stopwords>,
    /// Stemmer override (`None` = language default).
    pub stemmer: Option<Stemmer>,
    /// Drop tokens shorter than this (chars, always enforced, like Qdrant).
    pub min_token_len: Option<usize>,
    /// Drop tokens longer than this (chars; on the document path, like
    /// Qdrant — the prefix query path truncates instead).
    pub max_token_len: Option<usize>,
}

impl Default for Bm25TextConfig {
    /// Qdrant server defaults: `k1 = 1.2`, `b = 0.75`, `avg_len = 256`,
    /// word tokenizer, English, lowercase on, folding off, language
    /// stopwords/stemmer, no length limits.
    fn default() -> Self {
        Self {
            params: Bm25Params::default(),
            tokenizer: Tokenizer::Word,
            language: Language::English,
            lowercase: true,
            ascii_folding: false,
            stopwords: None,
            stemmer: None,
            min_token_len: None,
            max_token_len: None,
        }
    }
}

impl Bm25TextConfig {
    /// Resolve flat options into a validated config. `None` keeps the
    /// corresponding default; invalid names or numbers fail closed with
    /// `QQL-VALIDATION-CONFIG`.
    ///
    /// - `stopwords`: `None` = language default; `Some(list)` **replaces**
    ///   it with exactly `list` (empty disables filtering).
    /// - `stemmer`: `None` = language default; `Some("none")` disables;
    ///   `Some("<language>")` overrides.
    /// - `tokenizer`: `"multilingual"` parses here but fails at embed time
    ///   (see [`Tokenizer::Multilingual`]).
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        k1: Option<f64>,
        b: Option<f64>,
        avg_len: Option<f64>,
        language: Option<&str>,
        tokenizer: Option<&str>,
        lowercase: Option<bool>,
        ascii_folding: Option<bool>,
        stopwords: Option<Vec<String>>,
        stemmer: Option<&str>,
        min_token_len: Option<usize>,
        max_token_len: Option<usize>,
    ) -> Result<Self, QqlError> {
        Ok(Self {
            params: Bm25Params::resolve(k1, b, avg_len)?,
            tokenizer: match tokenizer {
                None => Tokenizer::Word,
                Some(name) => Tokenizer::parse(name)?,
            },
            language: match language {
                None => Language::English,
                Some(name) => Language::parse(name)?,
            },
            lowercase: lowercase.unwrap_or(true),
            ascii_folding: ascii_folding.unwrap_or(false),
            stopwords: stopwords.map(|custom| Stopwords {
                languages: Vec::new(),
                custom,
            }),
            stemmer: match stemmer {
                None => None,
                Some(name) => Some(Stemmer::parse(name)?),
            },
            min_token_len,
            max_token_len,
        })
    }

    /// Compile into an executable pipeline. Infallible: names already
    /// validated at parse/resolve time; remaining choices are total.
    pub fn pipeline(&self) -> Bm25Pipeline {
        let stemmer = match self.stemmer {
            Some(Stemmer::Disabled) => None,
            Some(Stemmer::Snowball(language)) => {
                language.stem_algorithm().map(SnowballStemmer::create)
            }
            None => self.language.stem_algorithm().map(SnowballStemmer::create),
        };
        // Mirror Qdrant's `StopwordsFilter`: entries are lowercased at build
        // time when `lowercase` is on, and matching runs post-normalization.
        let mut stopwords = HashSet::new();
        let mut insert = |word: &str| {
            if self.lowercase {
                stopwords.insert(word.to_lowercase());
            } else {
                stopwords.insert(word.to_string());
            }
        };
        match &self.stopwords {
            None => {
                for word in stopwords_for(self.language) {
                    insert(word);
                }
            }
            Some(selection) => {
                for language in &selection.languages {
                    for word in stopwords_for(*language) {
                        insert(word);
                    }
                }
                for word in &selection.custom {
                    insert(word.as_str());
                }
            }
        }
        Bm25Pipeline {
            params: self.params,
            tokenizer: self.tokenizer,
            lowercase: self.lowercase,
            ascii_folding: self.ascii_folding,
            stopwords,
            stemmer,
            min_token_len: self.min_token_len,
            max_token_len: self.max_token_len,
        }
    }
}

/// Compiled BM25 pipeline: build once per config, embed many texts.
///
/// Construct via [`Bm25TextConfig::pipeline`]. The default English pipeline
/// backing the [`crate::sparse`] free functions is shared process-wide.
pub struct Bm25Pipeline {
    params: Bm25Params,
    tokenizer: Tokenizer,
    lowercase: bool,
    ascii_folding: bool,
    stopwords: HashSet<String>,
    stemmer: Option<SnowballStemmer>,
    min_token_len: Option<usize>,
    max_token_len: Option<usize>,
}

impl Bm25Pipeline {
    /// Default text knobs with explicit numeric parameters.
    pub fn with_params(params: &Bm25Params) -> Self {
        Bm25TextConfig {
            params: *params,
            ..Bm25TextConfig::default()
        }
        .pipeline()
    }

    /// Qdrant's stage order: fold → lowercase → stopwords → stem → length.
    /// `check_max_len` is Qdrant's per-call flag: word/whitespace pass true
    /// on both paths; the prefix document path passes false (the n-gram loop
    /// bounds length instead); the prefix query path truncates afterwards.
    fn process_token<'a>(
        &self,
        raw: &'a str,
        is_query: bool,
        check_max_len: bool,
    ) -> Option<Cow<'a, str>> {
        if raw.is_empty() {
            return None;
        }
        let mut token: Cow<'a, str> = Cow::Borrowed(raw);
        if self.ascii_folding {
            token = fold_to_ascii_cow(token);
        }
        if self.lowercase {
            token = Cow::Owned(token.to_lowercase());
        }
        let prefix_query = is_query && self.tokenizer == Tokenizer::Prefix;
        if !prefix_query && self.stopwords.contains(token.as_ref()) {
            return None;
        }
        if let Some(stemmer) = self.stemmer.as_ref() {
            token = Cow::Owned(stemmer.stem(token.as_ref()).into_owned());
        }
        if self
            .min_token_len
            .is_some_and(|min| token.chars().count() < min)
        {
            return None;
        }
        if check_max_len
            && self
                .max_token_len
                .is_some_and(|max| token.chars().count() > max)
        {
            return None;
        }
        Some(token)
    }

    /// Iterate processed tokens (`is_query` selects the query path).
    fn for_each<F>(&self, text: &str, is_query: bool, mut f: F) -> Result<(), QqlError>
    where
        F: FnMut(&str),
    {
        match self.tokenizer {
            Tokenizer::Word => {
                for raw in text.split(|c: char| !c.is_alphanumeric()) {
                    if let Some(token) = self.process_token(raw, is_query, true) {
                        f(token.as_ref());
                    }
                }
            }
            Tokenizer::Whitespace => {
                for raw in text.split_whitespace() {
                    if let Some(token) = self.process_token(raw, is_query, true) {
                        f(token.as_ref());
                    }
                }
            }
            Tokenizer::Prefix => {
                if is_query {
                    self.for_each_prefix_query(text, &mut f);
                } else {
                    self.for_each_prefix_doc(text, &mut f);
                }
            }
            Tokenizer::Multilingual => {
                return Err(config_error(
                    "bm25 tokenizer \"multilingual\" needs script-aware segmentation (charabia/vaporetto), which is not compiled in; use \"word\" or \"whitespace\"".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Document path: expand every n-gram in `min..=max` (Qdrant's
    /// `PrefixTokenizer::tokenize`; `max` unbounded emits up to the full word
    /// and always emits the full word last).
    fn for_each_prefix_doc<F>(&self, text: &str, mut f: F)
    where
        F: FnMut(&str),
    {
        let min_ngram = self.min_token_len.unwrap_or(1);
        let max_ngram = self.max_token_len.unwrap_or(usize::MAX);
        for raw in text.split(|c: char| !c.is_alphanumeric()) {
            let Some(word) = self.process_token(raw, false, false) else {
                continue;
            };
            for n in min_ngram..=max_ngram {
                match word.char_indices().map(|(i, _)| i).nth(n) {
                    Some(end) => f(&word[..end]),
                    None => {
                        f(word.as_ref());
                        break;
                    }
                }
            }
        }
    }

    /// Query path: longest n-gram only, no stopwords (Qdrant's
    /// `PrefixTokenizer::tokenize_query`).
    fn for_each_prefix_query<F>(&self, text: &str, mut f: F)
    where
        F: FnMut(&str),
    {
        let max_ngram = self.max_token_len.unwrap_or(usize::MAX);
        for raw in text.split(|c: char| !c.is_alphanumeric()) {
            if raw.is_empty() {
                continue;
            }
            // No stopwords and no max-as-filter here: over-long words
            // truncate to `max_ngram` below instead of dropping.
            let Some(word) = self.process_token(raw, true, false) else {
                continue;
            };
            match word.char_indices().map(|(i, _)| i).nth(max_ngram) {
                Some(end) => f(&word[..end]),
                None => f(word.as_ref()),
            }
        }
    }

    /// Processed document tokens (post-pipeline, in order, duplicates kept).
    /// Used by the `avg_len` estimator so the estimate measures exactly the
    /// `doc_len` the TF formula consumes.
    pub fn doc_tokens(&self, text: &str) -> Result<Vec<String>, QqlError> {
        let mut tokens = Vec::new();
        self.for_each(text, false, |token| {
            tokens.push(token.to_string());
        })?;
        Ok(tokens)
    }

    /// Processed query tokens (query path: prefix keeps the longest n-gram
    /// only and skips stopwords; other tokenizers match the document path).
    pub(crate) fn for_each_query<F>(&self, text: &str, f: F) -> Result<(), QqlError>
    where
        F: FnMut(&str),
    {
        self.for_each(text, true, f)
    }

    /// Post-pipeline token count of one document (the formula's `doc_len`).
    pub fn token_count(&self, text: &str) -> Result<usize, QqlError> {
        let mut count = 0;
        self.for_each(text, false, |_| {
            count += 1;
        })?;
        Ok(count)
    }

    /// Embed query text: unique token IDs (sorted) with unit weights —
    /// identical to Qdrant's `qdrant/bm25` query embedding.
    pub fn embed_query(&self, text: &str) -> Result<SparseVector, QqlError> {
        let mut indices = Vec::with_capacity(text.len() / 6 + 1);
        self.for_each_query(text, |token| {
            indices.push(token_id(token));
        })?;
        if indices.is_empty() {
            return Ok(SparseVector::default());
        }
        indices.sort_unstable();
        indices.dedup();
        let values = vec![1.0; indices.len()];
        Ok(SparseVector { indices, values })
    }

    /// Embed document text with this pipeline's validated [`Bm25Params`].
    ///
    /// Frequencies count per token ID: on the rare murmur3 collision two
    /// terms merge into one dimension with summed counts, keeping output
    /// deterministic (the server counts per string, so collided IDs carry no
    /// cross-implementation contract — same caveat as before).
    pub fn embed_document(&self, text: &str) -> Result<SparseVector, QqlError> {
        self.embed_document_with(
            text,
            self.params.k1(),
            self.params.b(),
            self.params.avg_len(),
        )
    }

    /// Embed with explicit parameters, used as given (no validation — the
    /// caller sanitizes, mirroring [`crate::sparse::embed_document_with`]).
    /// The formula is the same fused op order as [`Bm25Pipeline::embed_document`].
    pub(crate) fn embed_document_with(
        &self,
        text: &str,
        k1: f64,
        b: f64,
        avgdl: f64,
    ) -> Result<SparseVector, QqlError> {
        let mut token_ids: Vec<u32> = Vec::with_capacity(text.len() / 6 + 1);
        self.for_each(text, false, |token| {
            token_ids.push(token_id(token));
        })?;
        if token_ids.is_empty() {
            return Ok(SparseVector::default());
        }
        let doc_len = token_ids.len() as f64;
        // Same fused operation order as Qdrant `lib/bm25`, so weights agree
        // bit-for-bit, not just algebraically: `n * (k1 + 1)` over
        // `k1.mul_add(1 - b + b * doc_len / avgdl, n)`.
        let k1p1 = k1 + 1.0;
        let norm = 1.0 - b + b * doc_len / avgdl;
        token_ids.sort_unstable();
        let mut indices = Vec::with_capacity(token_ids.len());
        let mut values = Vec::with_capacity(token_ids.len());
        let mut i = 0;
        while i < token_ids.len() {
            let id = token_ids[i];
            let mut count = 1u32;
            while i + 1 < token_ids.len() && token_ids[i + 1] == id {
                count += 1;
                i += 1;
            }
            indices.push(id);
            let n = count as f64;
            values.push((n * k1p1 / k1.mul_add(norm, n)) as f32);
            i += 1;
        }
        Ok(SparseVector { indices, values })
    }
}

static DEFAULT_PIPELINE: LazyLock<Bm25Pipeline> =
    LazyLock::new(|| Bm25TextConfig::default().pipeline());

/// Default English pipeline backing the [`crate::sparse`] free functions.
pub fn default_pipeline() -> &'static Bm25Pipeline {
    &DEFAULT_PIPELINE
}

/// Mean post-pipeline token count over sampled document texts — the
/// estimator for a corpus-true `avg_len`.
///
/// "Real data" in one function: pass the actual field texts (e.g. from a
/// `SCROLL` sample) and get the average `doc_len` the TF formula consumes,
/// measured with the same pipeline that will embed the writes. Returns
/// `None` when the sample holds no documents or no tokens at all (an empty
/// corpus has no meaningful average — keep the default instead of
/// dividing by zero or storing `avg_len = 0`, which validation rejects).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvgLenEstimate {
    /// Mean post-pipeline tokens per document (`> 0` when returned).
    pub mean: f64,
    /// Documents measured.
    pub docs: usize,
}

/// Estimate a corpus-true `avg_len` from sampled document texts.
///
/// Measures each text with `pipeline.token_count` (the same `doc_len` the
/// TF formula consumes) and returns the mean. Returns `None` when the
/// sample holds no documents or no tokens at all — an empty corpus has no
/// meaningful average, so callers should keep the default instead of
/// storing `avg_len = 0` (which [`Bm25Params`] validation rejects).
pub fn estimate_avg_len<'a, I>(
    texts: I,
    pipeline: &Bm25Pipeline,
) -> Result<Option<AvgLenEstimate>, QqlError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut docs = 0usize;
    let mut total = 0usize;
    for text in texts {
        docs += 1;
        total += pipeline.token_count(text)?;
    }
    if docs == 0 || total == 0 {
        return Ok(None);
    }
    Ok(Some(AvgLenEstimate {
        mean: total as f64 / docs as f64,
        docs,
    }))
}
