//! nqql-edge — local QQL execution via qdrant-edge + fastembed.
//!
//! Zero network.  No Qdrant server required.  Parser + in-process HNSW.
//!
//! ```js
//! const nqql = require('nqql-edge');
//!
//! // ── Parser (same API as nqql) ──
//! const stmt = nqql.parse("QUERY 'hello' FROM docs LIMIT 10")[0];
//! const tokens = nqql.tokenize("QUERY 'test' FROM docs");
//! const plan = nqql.explain("QUERY 'hello' FROM docs LIMIT 10");
//!
//! // ── Edge execution ──
//! const exec = nqql.localExecutor("./qdrant_data");
//! const result = await exec.execute("QUERY 'hello' FROM docs LIMIT 10");
//! ```
//!
//! The parser/parameter/execution logic lives in `nqql-common`, shared with
//! `nqql` so the two SDKs cannot drift; this crate keeps only the `#[napi]`
//! wrappers plus the edge client construction and one-shot helpers.

use napi_derive::napi;

use nqql_common as common;

// ═══════════════════════════════════════════════════════════════════
//  Stmt class — mirrors nqql.Stmt
// ═══════════════════════════════════════════════════════════════════

#[napi]
#[derive(Clone)]
pub struct Stmt {
    inner: qql_core::ast::Stmt,
}

#[napi]
impl Stmt {
    /// Parse a QQL string into a Stmt handle.
    #[napi(constructor, catch_unwind)]
    pub fn new(input: String) -> napi::Result<Self> {
        Ok(Stmt {
            inner: common::stmt_parse(&input).map_err(common::to_napi_err)?,
        })
    }

    #[napi(catch_unwind)]
    pub fn inject_filter(
        &mut self,
        field: String,
        op: String,
        value: serde_json::Value,
    ) -> napi::Result<()> {
        common::stmt_inject_filter(&mut self.inner, &field, &op, value).map_err(common::to_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn to_object(&self) -> napi::Result<serde_json::Value> {
        serde_json::to_value(&self.inner).map_err(common::serde_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn to_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner).map_err(common::serde_napi_err)
    }

    /// QQL `SHARD '…'` routing key (request-level). Prefer the clause in QQL.
    #[napi(getter, catch_unwind)]
    pub fn shard_key(&self) -> Option<String> {
        self.inner.shard_key().map(str::to_owned)
    }

    #[napi(setter, catch_unwind)]
    pub fn set_shard_key(&mut self, key: Option<String>) -> napi::Result<()> {
        if !self.inner.set_shard_key(key) {
            return Err(napi::Error::from_reason(
                "cannot set shardKey on statement type that does not support sharding (e.g. DDL statements)",
            ));
        }
        Ok(())
    }

    /// Bind parameters into this statement and return a new bound Stmt.
    #[napi(catch_unwind)]
    pub fn bind(&self, params: Option<serde_json::Value>) -> napi::Result<Self> {
        Ok(Stmt {
            inner: common::stmt_bind(&self.inner, params.as_ref()).map_err(common::to_napi_err)?,
        })
    }

    /// Format statement as canonical, re-parseable QQL (mirrors Python `str(stmt)`).
    #[allow(clippy::inherent_to_string)]
    #[napi(catch_unwind, js_name = "toString")]
    pub fn to_string(&self) -> String {
        common::stmt_full(&self.inner)
    }

    /// Format statement as a human-readable preview (mirrors Python `repr(stmt)`):
    /// long vector literals are truncated, so the output may not re-parse.
    #[napi(catch_unwind, js_name = "toReadableString")]
    pub fn to_readable_string(&self) -> String {
        common::stmt_readable(&self.inner)
    }

    /// Compile this Stmt AST directly into its transport route without re-parsing.
    /// Optionally accepts `params` to bind before compiling.
    #[napi(catch_unwind)]
    pub fn compile_route(
        &self,
        params: Option<serde_json::Value>,
    ) -> napi::Result<serde_json::Value> {
        common::stmt_compile_route(&self.inner, params.as_ref()).map_err(common::to_napi_err)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Parser functions (identical to nqql — shared via nqql-common)
// ═══════════════════════════════════════════════════════════════════

#[napi(catch_unwind)]
pub fn parse_all(input: String) -> napi::Result<Vec<Stmt>> {
    Ok(common::parse_all(&input)
        .map_err(common::to_napi_err)?
        .into_iter()
        .map(|inner| Stmt { inner })
        .collect())
}

/// Fast JSON-only parse — returns a JSON string of the AST array.
/// Bypasses V8 Stmt object allocation entirely (~2× throughput).
/// Ideal for HTTP/IPC forwarding.
#[napi(js_name = parseAllJson, catch_unwind)]
pub fn parse_all_json(input: String) -> napi::Result<String> {
    common::parse_all_json(&input).map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn is_valid(input: String) -> bool {
    common::is_valid(&input)
}

#[napi(catch_unwind)]
pub fn inject_filter(
    query: String,
    field: String,
    op: String,
    value: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    common::inject_filter(&query, &field, &op, value).map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn tokenize(input: String) -> napi::Result<serde_json::Value> {
    common::tokenize(&input).map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn compile_query(
    input: String,
    params: Option<serde_json::Value>,
) -> napi::Result<serde_json::Value> {
    common::compile_query(&input, params.as_ref()).map_err(common::to_napi_err)
}

// ═══════════════════════════════════════════════════════════════════
//  Explain (standalone, no client needed)
// ═══════════════════════════════════════════════════════════════════

#[napi(catch_unwind)]
pub fn explain(query: String) -> napi::Result<String> {
    common::explain(&query).map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn explain_stmt(stmt: &Stmt) -> napi::Result<String> {
    Ok(common::explain_stmt(&stmt.inner))
}

// ═══════════════════════════════════════════════════════════════════
//  Edge Client — wraps qql-edge Executor
// ═══════════════════════════════════════════════════════════════════

#[napi(js_name = "Client")]
pub struct JsClient {
    inner: qql::executor::Executor,
    closed: std::sync::atomic::AtomicBool,
}

#[napi]
impl JsClient {
    /// Constructor required by napi-rs class registry.  Always throws —
    /// use `localExecutor()` or `httpExecutor()` to obtain a Client.
    #[napi(constructor, catch_unwind)]
    pub fn new() -> napi::Result<Self> {
        Err(napi::Error::from_reason(
            "Client must be created via localExecutor() or httpExecutor()",
        ))
    }

    /// Execute a QQL query string, a Stmt, or an array of either.
    /// Multi-statement strings (semicolons) and arrays are auto-batched.
    /// Returns a stable ExecutionReport JSON string for the JavaScript wrapper
    /// to deserialize into an object.
    #[napi(
        catch_unwind,
        ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue', params?: Record<string, any> | any[] }"
    )]
    pub async fn execute(
        &self,
        query: serde_json::Value,
        options: Option<serde_json::Value>,
    ) -> napi::Result<String> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(napi::Error::from_reason("client is closed"));
        }
        let report = common::execute::execute_dispatch(&self.inner, query, options.as_ref())
            .await
            .map_err(common::to_napi_err)?;
        serde_json::to_string(&report).map_err(common::serde_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn explain(&self, query: String) -> napi::Result<String> {
        qql::executor::Executor::explain(&query).map_err(common::to_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn explain_stmt(&self, stmt: &Stmt) -> napi::Result<String> {
        qql::executor::Executor::explain_node(&stmt.inner).map_err(common::to_napi_err)
    }

    /// Compile a QQL query to its transport route (non-executing).
    #[napi(catch_unwind)]
    pub fn compile(
        &self,
        query: String,
        params: Option<serde_json::Value>,
    ) -> napi::Result<serde_json::Value> {
        common::compile_query(&query, params.as_ref()).map_err(common::to_napi_err)
    }

    /// Flush and release edge storage. Idempotent; execution after close is
    /// rejected instead of silently reopening shards.
    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        if self.closed.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Ok(());
        }
        self.inner.close().await.map_err(common::to_napi_err)
    }
}

#[cfg(any(feature = "fastembed-local", feature = "http-embedding"))]
impl JsClient {
    fn from_executor(exec: qql::executor::Executor) -> Self {
        Self {
            inner: exec,
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Executor constructors — edge-only, no REST/gRPC
// ═══════════════════════════════════════════════════════════════════

/// Options for `localExecutor(dataDir, options?)`.
///
/// The JS wrapper also accepts a bare boolean as the second argument for
/// backwards compatibility (`localExecutor(dir, false)` → `onDiskPayload: false`).
#[cfg(feature = "fastembed-local")]
#[napi(object)]
#[derive(Default)]
pub struct LocalExecutorOptions {
    /// Store payloads on disk (default `true`).
    pub on_disk_payload: Option<bool>,
    /// Local ONNX dense model. Accepts enum names (`BGESmallENV15`), HF codes
    /// (`Xenova/bge-small-en-v1.5`), or short aliases (`bge-small-en-v1.5`).
    /// Default: `BGESmallENV15` (384-d).
    pub model: Option<String>,
    /// Offline sparse model (SPLADE or BGE-M3 via SparseTextEmbedding), e.g. `"splade"`.
    /// `None` → local wire-compatible BM25 (Qdrant `qdrant/bm25`-identical
    /// token IDs) for sparse requests.
    pub sparse_model: Option<String>,
    /// Offline multivector model (BGE-M3 ColBERT), e.g. `"bge-m3"`.
    pub multi_model: Option<String>,
    /// Offline CLIP vision model, e.g. `"clip-vision"` / `"ClipVitB32"`.
    pub image_model: Option<String>,
    /// Offline cross-encoder, e.g. `"bge-reranker-base"`.
    pub reranker_model: Option<String>,
    /// Override model cache directory (default: fastembed / HF cache).
    pub cache_dir: Option<String>,
    /// Show HuggingFace download progress (default `false`).
    pub show_download_progress: Option<bool>,
}

/// Create a fully-local edge executor backed by fastembed-rs and qdrant-edge.
///
/// No network calls for inference — embedding runs on-device via ONNX.
/// Models are downloaded from HuggingFace on first use and cached locally.
/// The npm package ships NO model files.
///
/// ```js
/// // defaults
/// const exec = localExecutor("./data");
/// // pick model + cache
/// const exec = localExecutor("./data", {
///   onDiskPayload: false,
///   model: "AllMiniLML6V2",
///   cacheDir: "/var/cache/fastembed",
/// });
/// ```
#[cfg(feature = "fastembed-local")]
#[napi(js_name = localExecutor, catch_unwind)]
pub fn local_executor(
    data_dir: String,
    options: Option<LocalExecutorOptions>,
) -> napi::Result<JsClient> {
    let opts = options.unwrap_or_default();
    let exec = qql_edge::local_executor_with_options(
        data_dir,
        qql_edge::LocalExecutorOptions {
            on_disk_payload: opts.on_disk_payload.unwrap_or(true),
            model: opts.model,
            sparse_model: opts.sparse_model,
            multi_model: opts.multi_model,
            image_model: opts.image_model,
            reranker_model: opts.reranker_model,
            cache_dir: opts.cache_dir.map(std::path::PathBuf::from),
            show_download_progress: opts.show_download_progress.unwrap_or(false),
        },
    )
    .map_err(common::to_napi_err)?;
    Ok(JsClient::from_executor(exec))
}

/// List dense text embedding models available for `localExecutor({ model })`.
///
/// Returns `[{ name, modelCode, dim, description }, ...]`.
#[cfg(feature = "fastembed-local")]
#[napi(js_name = listEmbeddingModels, catch_unwind)]
pub fn list_embedding_models() -> Vec<EmbeddingModelInfoJs> {
    qql_edge::list_embedding_models()
        .into_iter()
        .map(|m| EmbeddingModelInfoJs {
            name: m.name,
            model_code: m.model_code,
            dim: m.dim as u32,
            description: m.description,
            multi: m.multi,
            image: m.image,
        })
        .collect()
}

#[cfg(feature = "fastembed-local")]
#[napi(object)]
pub struct EmbeddingModelInfoJs {
    pub name: String,
    pub model_code: String,
    pub dim: u32,
    pub description: String,
    pub multi: bool,
    pub image: bool,
}

/// Create an edge executor that calls an external OpenAI-compatible embedding
/// endpoint instead of running fastembed locally.
///
/// Works with: OpenAI, Ollama (`/v1/embeddings`), Cohere, Together AI,
/// Mistral, and any other provider that follows the OpenAI embeddings spec.
///
/// - `url` — full URL, e.g. `"https://api.openai.com/v1/embeddings"`
/// - `embedKey` — Bearer token (use `""` for unauthenticated local providers)
/// - `embedModel` — model name, e.g. `"text-embedding-3-small"`
/// - `embedDim` — expected output dimension
///
/// `onDiskPayload` defaults to `true`.
#[cfg(feature = "http-embedding")]
#[napi(js_name = httpExecutor, catch_unwind)]
pub fn http_executor(
    data_dir: String,
    url: String,
    embed_key: String,
    embed_model: String,
    embed_dim: u32,
    on_disk_payload: Option<bool>,
) -> napi::Result<JsClient> {
    let on_disk = on_disk_payload.unwrap_or(true);
    let exec = qql_edge::http_executor(
        data_dir,
        on_disk,
        url,
        embed_key,
        embed_model,
        embed_dim as usize,
    )
    .map_err(common::to_napi_err)?;
    Ok(JsClient::from_executor(exec))
}

// ═══════════════════════════════════════════════════════════════════
//  Standalone execute (one-shot with a temporary client)
// ═══════════════════════════════════════════════════════════════════

#[cfg(feature = "fastembed-local")]
fn standalone_local_opts(options: Option<&serde_json::Value>) -> LocalExecutorOptions {
    LocalExecutorOptions {
        on_disk_payload: options
            .and_then(|o| o.get("onDiskPayload"))
            .and_then(|v| v.as_bool()),
        model: options
            .and_then(|o| o.get("model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        sparse_model: options
            .and_then(|o| o.get("sparseModel").or_else(|| o.get("sparse_model")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        multi_model: options
            .and_then(|o| o.get("multiModel").or_else(|| o.get("multi_model")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        image_model: options
            .and_then(|o| o.get("imageModel").or_else(|| o.get("image_model")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        reranker_model: options
            .and_then(|o| o.get("rerankerModel").or_else(|| o.get("reranker_model")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        cache_dir: options
            .and_then(|o| o.get("cacheDir"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        show_download_progress: options
            .and_then(|o| o.get("showDownloadProgress"))
            .and_then(|v| v.as_bool()),
    }
}

/// Build the one-shot edge client for the standalone `execute` / `executeStmt`
/// APIs.
///
/// HTTP embedding is selected whenever `options.embedUrl` is supplied — exactly
/// matching the long-lived `httpExecutor()` constructor and the standalone
/// `execute({ embedUrl })` path. When the `http-embedding` feature is not
/// compiled in, a supplied `embedUrl` returns an explicit error instead of being
/// silently ignored (falling back to the local ONNX model).
#[cfg(any(feature = "fastembed-local", feature = "http-embedding"))]
fn standalone_client(options: Option<&serde_json::Value>) -> napi::Result<JsClient> {
    let data_dir = options
        .and_then(|o| o.get("dataDir"))
        .and_then(|v| v.as_str())
        .unwrap_or("./qdrant_data");

    // Prefer http embedding if embedUrl is provided
    #[cfg(feature = "http-embedding")]
    {
        let on_disk = options
            .and_then(|o| o.get("onDiskPayload"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if let Some(embed_url) = options
            .and_then(|o| o.get("embedUrl"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            let embed_key = options
                .and_then(|o| o.get("embedKey"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let embed_model = options
                .and_then(|o| o.get("embedModel"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let embed_dim = options
                .and_then(|o| o.get("embedDim"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            return http_executor(
                data_dir.to_string(),
                embed_url.to_string(),
                embed_key.to_string(),
                embed_model.to_string(),
                embed_dim,
                Some(on_disk),
            );
        }
    }

    // Reject HTTP embedding options explicitly when the feature is absent,
    // instead of silently falling back to the local ONNX model.
    #[cfg(not(feature = "http-embedding"))]
    if options
        .and_then(|o| o.get("embedUrl"))
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
    {
        return Err(napi::Error::from_reason(
            "embedUrl requires the http-embedding feature (httpExecutor), which is not enabled in this build",
        ));
    }

    #[cfg(feature = "fastembed-local")]
    {
        local_executor(data_dir.to_string(), Some(standalone_local_opts(options)))
    }

    #[cfg(not(feature = "fastembed-local"))]
    Err(napi::Error::from_reason(
        "no embedding backend is enabled: build nqql-edge with fastembed-local and/or http-embedding",
    ))
}

/// Execute a pre-parsed Stmt directly via a new temporary edge client.
///
/// HTTP embedding is selected when `options.embedUrl` is supplied, matching the
/// standalone `execute` API; otherwise a local fastembed executor is used.
///
/// **Warning:** each call initialises the embedding backend (ONNX model load for
/// the local path, HTTP client for the `embedUrl` path). Prefer a long-lived
/// `localExecutor()` / `httpExecutor()` Client for anything beyond one-shots.
#[cfg(any(feature = "fastembed-local", feature = "http-embedding"))]
#[napi(
    catch_unwind,
    ts_args_type = "stmt: Stmt, options?: { onError?: 'stop' | 'continue'; dataDir?: string; onDiskPayload?: boolean; model?: string; cacheDir?: string; showDownloadProgress?: boolean; embedUrl?: string; embedKey?: string; embedModel?: string; embedDim?: number }"
)]
pub async fn execute_stmt(stmt: &Stmt, options: Option<serde_json::Value>) -> napi::Result<String> {
    let client = standalone_client(options.as_ref())?;
    let mut inner = stmt.inner.clone();
    if let Some(p) = options.as_ref().and_then(|o| o.get("params")) {
        let plan =
            qql_core::params_json::plan_statement_params(p, 1).map_err(common::to_napi_err)?;
        qql_core::params_json::bind_stmt_with_params(
            &mut inner,
            qql_core::params_json::param_for(&plan, 0),
        )
        .map_err(common::to_napi_err)?;
    }
    let resp = client
        .inner
        .execute_node(inner)
        .await
        .map_err(common::to_napi_err)?;
    let report = qql::executor::ExecutionReport::single(resp);
    client.inner.close().await.map_err(common::to_napi_err)?;
    serde_json::to_string(&report).map_err(common::serde_napi_err)
}

#[cfg(all(feature = "fastembed-local", not(feature = "http-embedding")))]
#[napi(
    catch_unwind,
    ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue'; params?: Record<string, any> | any[]; dataDir?: string; onDiskPayload?: boolean; model?: string; cacheDir?: string; showDownloadProgress?: boolean }"
)]
pub async fn execute(
    query: serde_json::Value,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let client = standalone_client(options.as_ref())?;
    let report = common::execute::execute_dispatch(&client.inner, query, options.as_ref())
        .await
        .map_err(common::to_napi_err)?;
    client.inner.close().await.map_err(common::to_napi_err)?;
    serde_json::to_string(&report).map_err(common::serde_napi_err)
}

#[cfg(feature = "http-embedding")]
#[napi(
    catch_unwind,
    ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue'; params?: Record<string, any> | any[]; dataDir?: string; onDiskPayload?: boolean; model?: string; cacheDir?: string; showDownloadProgress?: boolean; embedUrl?: string; embedKey?: string; embedModel?: string; embedDim?: number }"
)]
pub async fn execute(
    query: serde_json::Value,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let client = standalone_client(options.as_ref())?;
    let report = common::execute::execute_dispatch(&client.inner, query, options.as_ref())
        .await
        .map_err(common::to_napi_err)?;
    client.inner.close().await.map_err(common::to_napi_err)?;
    serde_json::to_string(&report).map_err(common::serde_napi_err)
}

/// Substitute `:name` (object) or `?` (array) placeholders into a query string.
/// Without `params`, the query is returned unchanged. With `truncateVectors`,
/// long vector literals render as `[0.1, 0.2, ... (N dims)]` for previews.
/// (Stmt inputs are handled by the JS wrapper, which routes to `Stmt.bind`.)
#[napi(
    catch_unwind,
    ts_args_type = "query: string, params?: Record<string, any> | any[], options?: { truncateVectors?: boolean }"
)]
pub fn bind(
    query: String,
    params: Option<serde_json::Value>,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let truncate = options
        .as_ref()
        .and_then(|o| o.get("truncateVectors"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match params {
        Some(p) => {
            let plan =
                qql_core::params_json::plan_statement_params(&p, 1).map_err(common::to_napi_err)?;
            qql_core::params_json::bind_str_with_params(
                &query,
                qql_core::params_json::param_for(&plan, 0),
                truncate,
            )
            .map_err(common::to_napi_err)
        }
        None => Ok(query),
    }
}

#[cfg(test)]
#[cfg(feature = "fastembed-local")]
mod tests;
