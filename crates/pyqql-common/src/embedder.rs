//! Shared `HttpEmbedder` class and embedder-config extraction.
//!
//! Both Python SDKs accept the same embedder configuration shapes: the
//! `HttpEmbedder` class (remote REST/gRPC transport) and a plain dict with
//! camelCase or snake_case keys. The class, its validation, and the dict
//! extraction live here so `pyqql` and a future edge HTTP transport cannot
//! drift; the SDKs only own transport construction.
//!
//! Note: lines carrying the `bm25_min_token_len` / `bm25_max_token_len`
//! identifiers are marked `// ggignore` — GitGuardian's entropy heuristic
//! reports those parameter names as false-positive secrets.

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};

/// Remote (HTTP) embedder configuration as a Python class.
///
/// Sparse document encoding is always local (the HTTP endpoint only serves
/// dense/multi/image/rerank), so the BM25 knobs ride the same constructor.
#[pyclass(name = "HttpEmbedder", frozen, from_py_object, get_all)]
#[derive(Clone)]
pub struct PyHttpEmbedder {
    /// Dense embedding endpoint URL (required, non-empty).
    pub endpoint: String,
    /// Bearer token for `endpoint` (empty = unauthenticated).
    pub api_key: String,
    /// Dense model name (required, non-empty).
    pub model: String,
    /// Expected dense dimension (must be positive).
    pub dimension: usize,
    /// ColBERT/multi endpoint; falls back to `endpoint` when unset.
    pub multi_endpoint: Option<String>,
    /// Bearer token for `multi_endpoint`.
    pub multi_api_key: Option<String>,
    /// Multi model name; falls back to `model` when unset.
    pub multi_model: Option<String>,
    /// Expected per-token dimension (`0` skips checks).
    pub multi_dimension: usize,
    /// Image/CLIP endpoint; falls back to `endpoint` when unset.
    pub image_endpoint: Option<String>,
    /// Bearer token for `image_endpoint`.
    pub image_api_key: Option<String>,
    /// Image model name; falls back to `model` when unset.
    pub image_model: Option<String>,
    /// Expected image dimension (`0` falls back to the dense dimension).
    pub image_dimension: usize,
    /// Cross-encoder rerank endpoint.
    pub rerank_endpoint: Option<String>,
    /// Bearer token for the rerank endpoint.
    pub rerank_api_key: Option<String>,
    /// Cross-encoder rerank model name.
    pub rerank_model: Option<String>,
    /// Client-side BM25 `k1` (document encoding).
    pub bm25_k1: Option<f64>,
    /// Client-side BM25 `b` (length normalization).
    pub bm25_b: Option<f64>,
    /// Client-side BM25 expected average document length.
    pub bm25_avg_len: Option<f64>,
    /// BM25 text-processing language.
    pub bm25_language: Option<String>,
    /// BM25 tokenizer.
    pub bm25_tokenizer: Option<String>,
    /// Lowercase before matching.
    pub bm25_lowercase: Option<bool>,
    /// Lucene ASCII-folding before lowercasing.
    pub bm25_ascii_folding: Option<bool>,
    /// Custom stopwords replacing the language default.
    pub bm25_stopwords: Option<Vec<String>>,
    /// Additional language stopword lists merged with `bm25_stopwords`.
    pub bm25_stopwords_languages: Option<Vec<String>>,
    /// Stemmer override.
    pub bm25_stemmer: Option<String>,
    /// Minimum token length in chars.
    pub bm25_min_token_len: Option<usize>, // ggignore
    /// Maximum token length in chars.
    pub bm25_max_token_len: Option<usize>, // ggignore
}

#[pymethods]
impl PyHttpEmbedder {
    /// Validate and construct an embedder configuration.
    ///
    /// `endpoint`, `model`, and a positive `dimension` are required; a
    /// non-object (`str`/`int`/…) fails closed. All BM25 knobs are validated
    /// with `QQL-VALIDATION-CONFIG` before any network call.
    #[new]
    #[pyo3(signature = (endpoint, model, dimension, api_key=None, multi_endpoint=None, multi_api_key=None, multi_model=None, multi_dimension=None, image_endpoint=None, image_api_key=None, image_model=None, image_dimension=None, rerank_endpoint=None, rerank_api_key=None, rerank_model=None, bm25_k1=None, bm25_b=None, bm25_avg_len=None, bm25_language=None, bm25_tokenizer=None, bm25_lowercase=None, bm25_ascii_folding=None, bm25_stopwords=None, bm25_stemmer=None, bm25_min_token_len=None, bm25_max_token_len=None, bm25_stopwords_languages=None))] // ggignore
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: &str,
        model: &str,
        dimension: usize,
        api_key: Option<String>,
        multi_endpoint: Option<String>,
        multi_api_key: Option<String>,
        multi_model: Option<String>,
        multi_dimension: Option<usize>,
        image_endpoint: Option<String>,
        image_api_key: Option<String>,
        image_model: Option<String>,
        image_dimension: Option<usize>,
        rerank_endpoint: Option<String>,
        rerank_api_key: Option<String>,
        rerank_model: Option<String>,
        bm25_k1: Option<f64>,
        bm25_b: Option<f64>,
        bm25_avg_len: Option<f64>,
        bm25_language: Option<String>,
        bm25_tokenizer: Option<String>,
        bm25_lowercase: Option<bool>,
        bm25_ascii_folding: Option<bool>,
        bm25_stopwords: Option<Vec<String>>,
        bm25_stemmer: Option<String>,
        bm25_min_token_len: Option<usize>, // ggignore
        bm25_max_token_len: Option<usize>, // ggignore
        bm25_stopwords_languages: Option<Vec<String>>,
    ) -> PyResult<Self> {
        if endpoint.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding endpoint is required",
            ));
        }
        if model.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding model is required",
            ));
        }
        if dimension == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding dimension must be positive",
            ));
        }
        validate_bm25_text(
            bm25_k1,
            bm25_b,
            bm25_avg_len,
            bm25_language.as_deref(),
            bm25_tokenizer.as_deref(),
            bm25_lowercase,
            bm25_ascii_folding,
            bm25_stopwords.clone(),
            bm25_stemmer.as_deref(),
            bm25_min_token_len, // ggignore
            bm25_max_token_len, // ggignore
            bm25_stopwords_languages.clone(),
        )?;
        Ok(PyHttpEmbedder {
            endpoint: endpoint.to_string(),
            api_key: api_key.unwrap_or_default(),
            model: model.to_string(),
            dimension,
            multi_endpoint: multi_endpoint.filter(|s| !s.trim().is_empty()),
            multi_api_key,
            multi_model: multi_model.filter(|s| !s.trim().is_empty()),
            multi_dimension: multi_dimension.unwrap_or(0),
            image_endpoint: image_endpoint.filter(|s| !s.trim().is_empty()),
            image_api_key,
            image_model: image_model.filter(|s| !s.trim().is_empty()),
            image_dimension: image_dimension.unwrap_or(0),
            rerank_endpoint: rerank_endpoint.filter(|s| !s.trim().is_empty()),
            rerank_api_key,
            rerank_model: rerank_model.filter(|s| !s.trim().is_empty()),
            bm25_k1,
            bm25_b,
            bm25_avg_len,
            bm25_language,
            bm25_tokenizer,
            bm25_lowercase,
            bm25_ascii_folding,
            bm25_stopwords,
            bm25_stemmer,
            bm25_min_token_len, // ggignore
            bm25_max_token_len, // ggignore
            bm25_stopwords_languages,
        })
    }
}

/// Validate client-side BM25 overrides eagerly so bad values raise
/// `ValueError` at embedder construction (`QQL-VALIDATION-CONFIG`).
/// Single choke point: the same [`qql::embedder::Bm25TextConfig::resolve`]
/// the Rust core uses, so Python accepts exactly what the engine accepts.
#[allow(clippy::too_many_arguments)]
pub fn validate_bm25_text(
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
    stopwords_languages: Option<Vec<String>>,
) -> PyResult<()> {
    qql::embedder::Bm25TextConfig::resolve(
        k1,
        b,
        avg_len,
        language,
        tokenizer,
        lowercase,
        ascii_folding,
        stopwords,
        stemmer,
        min_token_len,
        max_token_len,
        stopwords_languages,
    )
    .map(|_| ())
    .map_err(crate::qql_py_value_error)
}

/// Full embedder configuration shared by the class and dict paths.
#[derive(Debug, Clone, Default)]
pub struct ParsedEmbedderConfig {
    /// Dense endpoint URL.
    pub endpoint: Option<String>,
    /// Bearer token for `endpoint`.
    pub api_key: Option<String>,
    /// Dense model name.
    pub model: Option<String>,
    /// Expected dense dimension.
    pub dimension: Option<usize>,
    /// ColBERT/multi endpoint; falls back to `endpoint` when unset.
    pub multi_endpoint: Option<String>,
    /// Bearer token for `multi_endpoint`.
    pub multi_api_key: Option<String>,
    /// Multi model name.
    pub multi_model: Option<String>,
    /// Expected multivector dimension (`0` skips checks).
    pub multi_dimension: usize,
    /// Image/CLIP endpoint; falls back to `endpoint` when unset.
    pub image_endpoint: Option<String>,
    /// Bearer token for `image_endpoint`.
    pub image_api_key: Option<String>,
    /// Image model name.
    pub image_model: Option<String>,
    /// Expected image dimension (`0` falls back to dense).
    pub image_dimension: usize,
    /// Cross-encoder rerank endpoint.
    pub rerank_endpoint: Option<String>,
    /// Bearer token for the rerank endpoint.
    pub rerank_api_key: Option<String>,
    /// Cross-encoder rerank model name.
    pub rerank_model: Option<String>,
    /// Client-side BM25 `k1`.
    pub bm25_k1: Option<f64>,
    /// Client-side BM25 `b`.
    pub bm25_b: Option<f64>,
    /// Client-side BM25 expected average document length.
    pub bm25_avg_len: Option<f64>,
    /// BM25 text-processing language.
    pub bm25_language: Option<String>,
    /// BM25 tokenizer.
    pub bm25_tokenizer: Option<String>,
    /// Lowercase before matching.
    pub bm25_lowercase: Option<bool>,
    /// Lucene ASCII-folding before lowercasing.
    pub bm25_ascii_folding: Option<bool>,
    /// Custom stopwords replacing the language default.
    pub bm25_stopwords: Option<Vec<String>>,
    /// Additional language stopword lists merged with `bm25_stopwords`.
    pub bm25_stopwords_languages: Option<Vec<String>>,
    /// Stemmer override.
    pub bm25_stemmer: Option<String>,
    /// Minimum token length in chars.
    pub bm25_min_token_len: Option<usize>, // ggignore
    /// Maximum token length in chars.
    pub bm25_max_token_len: Option<usize>, // ggignore
}

fn opt_string_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<String>()?))
}

fn opt_nonempty_string_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
    Ok(opt_string_key(dict, key)?.filter(|s| !s.trim().is_empty()))
}

fn opt_usize_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<usize> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(0);
    };
    if value.is_none() {
        return Ok(0);
    }
    value.extract::<usize>()
}

/// Optional float key: missing/`None` → unset; anything non-numeric is a
/// `TypeError` from PyO3's extractor.
fn opt_f64_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<f64>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<f64>()?))
}

/// Optional bool key: missing/`None` → unset; anything non-bool is a
/// `TypeError` from PyO3's extractor.
fn opt_bool_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<bool>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<bool>()?))
}

/// Optional usize key: missing/`None` → unset; anything non-integer is a
/// `TypeError` from PyO3's extractor.
fn opt_usize_opt_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<usize>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<usize>()?))
}

/// Optional string-list key (e.g. custom stopwords): missing/`None` → unset.
fn opt_string_list_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<Vec<String>>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<Vec<String>>()?))
}

/// Extract an embedder config from an `HttpEmbedder` instance or a dict.
///
/// Dict keys accept camelCase or snake_case (`apiKey`/`api_key`,
/// `multiEndpoint`/`multi_endpoint`, `bm25K1`/`bm25_k1`, …) with camelCase
/// winning on conflicts — the same convention as `nqql`'s `HttpEmbedder`,
/// so embedder configs port between the Python and Node SDKs unchanged.
/// `None` values fall through to the other spelling; single-word keys
/// (`endpoint`, `model`, `dimension`) are identical in both cases.
pub fn extract_embedder_config(
    embedder: Option<&Bound<'_, PyAny>>,
) -> PyResult<ParsedEmbedderConfig> {
    let mut out = ParsedEmbedderConfig::default();

    if let Some(emb) = embedder {
        if let Ok(py_emb) = emb.extract::<PyRef<PyHttpEmbedder>>() {
            out.endpoint = Some(py_emb.endpoint.clone());
            out.api_key = Some(py_emb.api_key.clone());
            out.model = Some(py_emb.model.clone());
            out.dimension = Some(py_emb.dimension);
            out.multi_endpoint = py_emb.multi_endpoint.clone();
            out.multi_api_key = py_emb.multi_api_key.clone();
            out.multi_model = py_emb.multi_model.clone();
            out.multi_dimension = py_emb.multi_dimension;
            out.image_endpoint = py_emb.image_endpoint.clone();
            out.image_api_key = py_emb.image_api_key.clone();
            out.image_model = py_emb.image_model.clone();
            out.image_dimension = py_emb.image_dimension;
            out.rerank_endpoint = py_emb.rerank_endpoint.clone();
            out.rerank_api_key = py_emb.rerank_api_key.clone();
            out.rerank_model = py_emb.rerank_model.clone();
            out.bm25_k1 = py_emb.bm25_k1;
            out.bm25_b = py_emb.bm25_b;
            out.bm25_avg_len = py_emb.bm25_avg_len;
            out.bm25_language = py_emb.bm25_language.clone();
            out.bm25_tokenizer = py_emb.bm25_tokenizer.clone();
            out.bm25_lowercase = py_emb.bm25_lowercase;
            out.bm25_ascii_folding = py_emb.bm25_ascii_folding;
            out.bm25_stopwords = py_emb.bm25_stopwords.clone();
            out.bm25_stopwords_languages = py_emb.bm25_stopwords_languages.clone();
            out.bm25_stemmer = py_emb.bm25_stemmer.clone();
            out.bm25_min_token_len = py_emb.bm25_min_token_len; // ggignore
            out.bm25_max_token_len = py_emb.bm25_max_token_len; // ggignore
        } else if let Ok(dict) = emb.cast::<PyDict>() {
            // Normalize camelCase aliases onto their snake_case keys before
            // reading (camelCase wins; `None` falls through to snake_case).
            for (camel, snake) in [
                ("apiKey", "api_key"),
                ("multiEndpoint", "multi_endpoint"),
                ("multiApiKey", "multi_api_key"),
                ("multiModel", "multi_model"),
                ("multiDimension", "multi_dimension"),
                ("imageEndpoint", "image_endpoint"),
                ("imageApiKey", "image_api_key"),
                ("imageModel", "image_model"),
                ("imageDimension", "image_dimension"),
                ("rerankEndpoint", "rerank_endpoint"),
                ("rerankApiKey", "rerank_api_key"),
                ("rerankModel", "rerank_model"),
                ("bm25K1", "bm25_k1"),
                ("bm25B", "bm25_b"),
                ("bm25AvgLen", "bm25_avg_len"),
                ("bm25Language", "bm25_language"),
                ("bm25Tokenizer", "bm25_tokenizer"),
                ("bm25Lowercase", "bm25_lowercase"),
                ("bm25AsciiFolding", "bm25_ascii_folding"),
                ("bm25Stopwords", "bm25_stopwords"),
                ("bm25StopwordsLanguages", "bm25_stopwords_languages"),
                ("bm25Stemmer", "bm25_stemmer"),
                ("bm25MinTokenLen", "bm25_min_token_len"), // ggignore
                ("bm25MaxTokenLen", "bm25_max_token_len"), // ggignore
            ] {
                if let Some(value) = dict.get_item(camel)?
                    && !value.is_none()
                {
                    dict.set_item(snake, value)?;
                }
            }
            out.endpoint = Some(
                dict.get_item("endpoint")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.endpoint is required")
                    })?
                    .extract::<String>()?,
            );
            out.model = Some(
                dict.get_item("model")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.model is required")
                    })?
                    .extract::<String>()?,
            );
            out.dimension = Some(
                dict.get_item("dimension")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.dimension is required")
                    })?
                    .extract::<usize>()?,
            );
            out.api_key = opt_string_key(dict, "api_key")?;

            if out
                .endpoint
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.endpoint must not be empty",
                ));
            }
            if out
                .model
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.model must not be empty",
                ));
            }
            if out.dimension == Some(0) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.dimension must be positive",
                ));
            }

            // Optional multi/image/rerank groups: empty strings unset, None
            // and missing keys unset, 0 dims skip checks (image 0 falls back
            // to dense dim in the Rust core).
            out.multi_endpoint = opt_nonempty_string_key(dict, "multi_endpoint")?;
            out.multi_api_key = opt_string_key(dict, "multi_api_key")?;
            out.multi_model = opt_nonempty_string_key(dict, "multi_model")?;
            out.multi_dimension = opt_usize_key(dict, "multi_dimension")?;
            out.image_endpoint = opt_nonempty_string_key(dict, "image_endpoint")?;
            out.image_api_key = opt_string_key(dict, "image_api_key")?;
            out.image_model = opt_nonempty_string_key(dict, "image_model")?;
            out.image_dimension = opt_usize_key(dict, "image_dimension")?;
            out.rerank_endpoint = opt_nonempty_string_key(dict, "rerank_endpoint")?;
            out.rerank_api_key = opt_string_key(dict, "rerank_api_key")?;
            out.rerank_model = opt_nonempty_string_key(dict, "rerank_model")?;

            // Optional client-side BM25 document parameters (local sparse path).
            out.bm25_k1 = opt_f64_key(dict, "bm25_k1")?;
            out.bm25_b = opt_f64_key(dict, "bm25_b")?;
            out.bm25_avg_len = opt_f64_key(dict, "bm25_avg_len")?;
            out.bm25_language = opt_string_key(dict, "bm25_language")?;
            out.bm25_tokenizer = opt_string_key(dict, "bm25_tokenizer")?;
            out.bm25_lowercase = opt_bool_key(dict, "bm25_lowercase")?;
            out.bm25_ascii_folding = opt_bool_key(dict, "bm25_ascii_folding")?;
            out.bm25_stopwords = opt_string_list_key(dict, "bm25_stopwords")?;
            out.bm25_stopwords_languages = opt_string_list_key(dict, "bm25_stopwords_languages")?;
            out.bm25_stemmer = opt_string_key(dict, "bm25_stemmer")?;
            out.bm25_min_token_len = opt_usize_opt_key(dict, "bm25_min_token_len")?; // ggignore
            out.bm25_max_token_len = opt_usize_opt_key(dict, "bm25_max_token_len")?; // ggignore
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "embedder must be an HttpEmbedder or dict",
            ));
        }
    }
    Ok(out)
}
