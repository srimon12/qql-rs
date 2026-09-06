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

use napi_derive::napi;
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_plan::routing;

/// Serialize a QqlError to JSON so the JS wrapper can extract structured fields.
fn to_napi_err(e: QqlError) -> napi::Error {
    let json = serde_json::to_string(&e).unwrap_or_else(|_| e.to_string());
    napi::Error::from_reason(json)
}

/// Convert a serde_json error to a napi error.
fn serde_napi_err(e: serde_json::Error) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

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
        let inner = Parser::parse(&input).map_err(to_napi_err)?;
        Ok(Stmt { inner })
    }

    #[napi(catch_unwind)]
    pub fn inject_filter(
        &mut self,
        field: String,
        op: String,
        value: serde_json::Value,
    ) -> napi::Result<()> {
        if op == "!=" || op == "neq" || op == "<>" {
            return Err(napi::Error::from_reason(
                "inject_filter does not support '!='; inject equality and wrap with NOT, or rewrite the query",
            ));
        }
        let cmp = match op.as_str() {
            "=" | "==" | "eq" => ComparisonOp::Eq,
            ">" | "gt" => ComparisonOp::Gt,
            ">=" | "gte" => ComparisonOp::Gte,
            "<" | "lt" => ComparisonOp::Lt,
            "<=" | "lte" => ComparisonOp::Lte,
            _ => {
                return Err(napi::Error::from_reason(format!(
                    "unsupported comparison operator '{op}' (use =, >, >=, <, <=)"
                )));
            }
        };
        let val = Value::from_json(value).map_err(to_napi_err)?;
        ast::inject_filter(&mut self.inner, &field, cmp, val).map_err(to_napi_err)?;
        Ok(())
    }

    #[napi(catch_unwind)]
    pub fn to_object(&self) -> napi::Result<serde_json::Value> {
        serde_json::to_value(&self.inner).map_err(serde_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn to_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner).map_err(serde_napi_err)
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
        let mut stmt = self.inner.clone();
        if let Some(ref p) = params {
            bind_stmt_json(&mut stmt, p).map_err(to_napi_err)?;
        }
        Ok(Stmt { inner: stmt })
    }

    /// Format statement as readable QQL string.
    #[allow(clippy::inherent_to_string)]
    #[napi(catch_unwind, js_name = "toString")]
    pub fn to_string(&self) -> String {
        qql_core::fmt::format_stmt_readable(&self.inner)
    }

    /// Compile this Stmt AST directly into its transport route without re-parsing.
    /// Optionally accepts `params` to bind before compiling.
    #[napi(catch_unwind)]
    pub fn compile_route(
        &self,
        params: Option<serde_json::Value>,
    ) -> napi::Result<serde_json::Value> {
        let mut stmt = self.inner.clone();
        if let Some(ref p) = params {
            bind_stmt_json(&mut stmt, p).map_err(to_napi_err)?;
        }
        let compiled = routing::compile_statement(&stmt).map_err(to_napi_err)?;
        let (method, path, payload) = match compiled.route {
            Some(route) => {
                let payload = route.body_json().unwrap_or(serde_json::Value::Null);
                (
                    serde_json::Value::String(route.method.as_str().into()),
                    serde_json::Value::String(route.path),
                    payload,
                )
            }
            None => (
                serde_json::Value::Null,
                serde_json::Value::Null,
                serde_json::Value::Null,
            ),
        };
        Ok(serde_json::json!({
            "stmt_type": compiled.stmt_type,
            "method": method,
            "path": path,
            "payload": payload,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Parser functions (identical to nqql)
// ═══════════════════════════════════════════════════════════════════

#[napi(catch_unwind)]
pub fn parse_all(input: String) -> napi::Result<Vec<Stmt>> {
    let stmts = Parser::parse_all(&input).map_err(to_napi_err)?;
    Ok(stmts.into_iter().map(|s| Stmt { inner: s }).collect())
}

/// Fast JSON-only parse — returns a JSON string of the AST array.
/// Bypasses V8 Stmt object allocation entirely (~2× throughput).
/// Ideal for HTTP/IPC forwarding.
#[napi(js_name = parseAllJson, catch_unwind)]
pub fn parse_all_json(input: String) -> napi::Result<String> {
    let stmts = Parser::parse_all(&input).map_err(to_napi_err)?;
    serde_json::to_string(&stmts).map_err(serde_napi_err)
}

#[napi(catch_unwind)]
pub fn is_valid(input: String) -> bool {
    Parser::parse_all(&input).is_ok()
}

#[napi(catch_unwind)]
pub fn inject_filter(
    query: String,
    field: String,
    op: String,
    value: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    if op == "!=" || op == "neq" || op == "<>" {
        return Err(napi::Error::from_reason(
            "inject_filter does not support '!='; inject equality and wrap with NOT, or rewrite the query",
        ));
    }
    let cmp = match op.as_str() {
        "=" | "==" | "eq" => ComparisonOp::Eq,
        ">" | "gt" => ComparisonOp::Gt,
        ">=" | "gte" => ComparisonOp::Gte,
        "<" | "lt" => ComparisonOp::Lt,
        "<=" | "lte" => ComparisonOp::Lte,
        _ => {
            return Err(napi::Error::from_reason(format!(
                "unsupported comparison operator '{op}' (use =, >, >=, <, <=)"
            )));
        }
    };
    let val = Value::from_json(value).map_err(to_napi_err)?;
    let mut stmt = Parser::parse(&query).map_err(to_napi_err)?;
    ast::inject_filter(&mut stmt, &field, cmp, val).map_err(to_napi_err)?;
    serde_json::to_value(&stmt).map_err(serde_napi_err)
}

#[napi(catch_unwind)]
pub fn tokenize(input: String) -> napi::Result<serde_json::Value> {
    #[derive(serde::Serialize)]
    struct TokenView<'a> {
        kind: &'a str,
        text: &'a str,
        pos: usize,
        end: usize,
        len: usize,
    }

    let lexer = Lexer::new(&input);
    let mut tokens = Vec::with_capacity(input.len() / 4 + 1);
    for token_result in lexer {
        let token =
            token_result.map_err(|e| napi::Error::new(napi::Status::InvalidArg, e.to_string()))?;
        tokens.push(TokenView {
            kind: token.kind.as_str(),
            text: token.text,
            pos: token.span.start,
            end: token.span.end,
            len: token.span.end.saturating_sub(token.span.start),
        });
    }
    serde_json::to_value(&tokens).map_err(|e| {
        napi::Error::new(
            napi::Status::GenericFailure,
            format!("failed to serialize tokens: {}", e),
        )
    })
}

#[napi(catch_unwind)]
pub fn compile_query(input: String) -> napi::Result<serde_json::Value> {
    let stmt = Parser::parse(&input).map_err(to_napi_err)?;
    let compiled = routing::compile_statement(&stmt).map_err(to_napi_err)?;
    let (method, path, payload) = match compiled.route {
        Some(route) => {
            let payload = route.body_json().unwrap_or(serde_json::Value::Null);
            (
                serde_json::Value::String(route.method.as_str().into()),
                serde_json::Value::String(route.path),
                payload,
            )
        }
        None => (
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
    };
    Ok(serde_json::json!({
        "stmt_type": compiled.stmt_type,
        "method": method,
        "path": path,
        "payload": payload,
    }))
}

// ═══════════════════════════════════════════════════════════════════
//  Explain (standalone, no client needed)
// ═══════════════════════════════════════════════════════════════════

#[napi(catch_unwind)]
pub fn explain(query: String) -> napi::Result<String> {
    qql_core::explain::explain(&query).map_err(to_napi_err)
}

#[napi(catch_unwind)]
pub fn explain_stmt(stmt: &Stmt) -> napi::Result<String> {
    Ok(qql_core::explain::explain_node(&stmt.inner))
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
        let on_error = options
            .as_ref()
            .and_then(|o| o.get("onError"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "continue" => qql::executor::OnError::Continue,
                _ => qql::executor::OnError::Stop,
            })
            .unwrap_or(qql::executor::OnError::Stop);
        let params = options.as_ref().and_then(|o| o.get("params"));

        let report = match &query {
            serde_json::Value::String(s) => {
                if let Some(serde_json::Value::Array(ref p_arr)) = params {
                    if let Ok(parsed_stmts) = Parser::parse_all(s) {
                        if parsed_stmts.len() > 1 && p_arr.len() == parsed_stmts.len() {
                            let mut bound_stmts = Vec::with_capacity(parsed_stmts.len());
                            for (i, mut stmt) in parsed_stmts.into_iter().enumerate() {
                                bind_stmt_json(&mut stmt, &p_arr[i]).map_err(to_napi_err)?;
                                bound_stmts.push(stmt);
                            }
                            let results = self
                                .inner
                                .execute_batch_nodes(
                                    bound_stmts,
                                    matches!(on_error, qql::executor::OnError::Stop),
                                )
                                .await
                                .map_err(to_napi_err)?;
                            let report = qql::executor::ExecutionReport::from_results(results);
                            return serde_json::to_string(&report).map_err(serde_napi_err);
                        }
                    }
                }
                let bound_query = if let Some(p) = params {
                    bind_json_params(s, p, false)?
                } else {
                    s.clone()
                };
                self.inner
                    .execute(&bound_query, on_error)
                    .await
                    .map_err(to_napi_err)?
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    qql::executor::ExecutionReport::empty()
                } else {
                    let scoped_params = if let Some(serde_json::Value::Array(ref p_arr)) = params {
                        if p_arr.len() == arr.len() {
                            Some(p_arr)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if arr[0].is_string() {
                        let mut bound_strs = Vec::with_capacity(arr.len());
                        for (i, v) in arr.iter().enumerate() {
                            let s = v.as_str().ok_or_else(|| {
                                napi::Error::from_reason("batch items must be strings")
                            })?;
                            let item_params = match scoped_params {
                                Some(scoped) => Some(&scoped[i]),
                                None => params,
                            };
                            let bound = if let Some(p) = item_params {
                                bind_json_params(s, p, false)?
                            } else {
                                s.to_string()
                            };
                            bound_strs.push(bound);
                        }
                        let refs: Vec<&str> = bound_strs.iter().map(|s| s.as_str()).collect();
                        self.inner
                            .execute_batch(&refs, on_error)
                            .await
                            .map_err(to_napi_err)?
                    } else {
                        let mut stmts = Vec::with_capacity(arr.len());
                        for (i, v) in arr.iter().enumerate() {
                            let mut s: ast::Stmt =
                                serde_json::from_value(v.clone()).map_err(|e| {
                                    napi::Error::from_reason(format!("invalid Stmt: {e}"))
                                })?;
                            let item_params = match scoped_params {
                                Some(scoped) => Some(&scoped[i]),
                                None => params,
                            };
                            if let Some(p) = item_params {
                                bind_stmt_json(&mut s, p).map_err(to_napi_err)?;
                            }
                            stmts.push(s);
                        }
                        let results = self
                            .inner
                            .execute_batch_nodes(
                                stmts,
                                matches!(on_error, qql::executor::OnError::Stop),
                            )
                            .await
                            .map_err(to_napi_err)?;
                        qql::executor::ExecutionReport::from_results(results)
                    }
                }
            }
            _ => {
                let mut s: ast::Stmt = serde_json::from_value(query)
                    .map_err(|e| napi::Error::from_reason(format!("invalid Stmt: {e}")))?;
                if let Some(p) = params {
                    bind_stmt_json(&mut s, p).map_err(to_napi_err)?;
                }
                let results = self
                    .inner
                    .execute_batch_nodes(vec![s], matches!(on_error, qql::executor::OnError::Stop))
                    .await
                    .map_err(to_napi_err)?;
                qql::executor::ExecutionReport::from_results(results)
            }
        };
        serde_json::to_string(&report).map_err(serde_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn explain(&self, query: String) -> napi::Result<String> {
        qql::executor::Executor::explain(&query).map_err(to_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn explain_stmt(&self, stmt: &Stmt) -> napi::Result<String> {
        qql::executor::Executor::explain_node(&stmt.inner).map_err(to_napi_err)
    }

    /// Compile a QQL query to its transport route (non-executing).
    #[napi(catch_unwind)]
    pub fn compile(&self, query: String) -> napi::Result<serde_json::Value> {
        crate::compile_query(query)
    }

    /// Flush and release edge storage. Idempotent; execution after close is
    /// rejected instead of silently reopening shards.
    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        if self.closed.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Ok(());
        }
        self.inner.close().await.map_err(to_napi_err)
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
    .map_err(to_napi_err)?;
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
    .map_err(to_napi_err)?;
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
        bind_stmt_json(&mut inner, p).map_err(to_napi_err)?;
    }
    let resp = client
        .inner
        .execute_node(inner)
        .await
        .map_err(to_napi_err)?;
    let report = qql::executor::ExecutionReport::single(resp);
    client.inner.close().await.map_err(to_napi_err)?;
    serde_json::to_string(&report).map_err(serde_napi_err)
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
    let report = client.execute(query, options).await?;
    client.inner.close().await.map_err(to_napi_err)?;
    Ok(report)
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
    let report = client.execute(query, options).await?;
    client.inner.close().await.map_err(to_napi_err)?;
    Ok(report)
}

fn flatten_json_object(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    out: &mut std::collections::HashMap<String, ast::Value>,
) -> Result<(), QqlError> {
    for (k, v) in obj {
        let full_key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        if let serde_json::Value::Object(nested) = v {
            flatten_json_object(nested, &full_key, out)?;
        }
        let val = ast::Value::from_json(v.clone())?;
        out.insert(full_key, val);
    }
    Ok(())
}

fn bind_json_params(
    query: &str,
    params: &serde_json::Value,
    truncate_vectors: bool,
) -> napi::Result<String> {
    match params {
        serde_json::Value::Object(obj) => {
            let mut map = std::collections::HashMap::new();
            flatten_json_object(obj, "", &mut map).map_err(to_napi_err)?;
            if truncate_vectors {
                qql_core::params::bind_named_readable(query, |k| map.get(k).cloned(), 2)
                    .map_err(to_napi_err)
            } else {
                qql_core::params::bind_named(query, |k| map.get(k).cloned()).map_err(to_napi_err)
            }
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<ast::Value> = arr
                .iter()
                .map(|v| ast::Value::from_json(v.clone()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_napi_err)?;
            if truncate_vectors {
                qql_core::params::bind_positional_readable(query, &items, 2).map_err(to_napi_err)
            } else {
                qql_core::params::bind_positional(query, &items).map_err(to_napi_err)
            }
        }
        _ => Err(napi::Error::from_reason(
            "params must be an object for named parameters (:name) or an array for positional parameters (?)",
        )),
    }
}

fn bind_stmt_json(stmt: &mut ast::Stmt, params: &serde_json::Value) -> Result<(), QqlError> {
    match params {
        serde_json::Value::Object(obj) => {
            let mut map = std::collections::HashMap::new();
            flatten_json_object(obj, "", &mut map)?;
            qql_core::params::bind_stmt(stmt, |k| map.get(k).cloned(), &[])
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<ast::Value> = arr
                .iter()
                .map(|v| ast::Value::from_json(v.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            qql_core::params::bind_stmt(stmt, |_| None, &items)
        }
        _ => Ok(()),
    }
}

/// Substitute `:name` (object) or `?` (array) placeholders into a query string.
#[napi(
    catch_unwind,
    ts_args_type = "query: string, params: Record<string, any> | any[], options?: { truncateVectors?: boolean }"
)]
pub fn bind(
    query: String,
    params: serde_json::Value,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let truncate = options
        .as_ref()
        .and_then(|o| o.get("truncateVectors"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    bind_json_params(&query, &params, truncate)
}

#[cfg(test)]
#[cfg(feature = "fastembed-local")]
mod tests;
