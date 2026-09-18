//! nqql — native Node.js bindings for the QQL parser and runtime.
//!
//! The parser/parameter/execution logic lives in `nqql-common`, shared with
//! `nqql-edge` so the two SDKs cannot drift; this crate keeps only the
//! `#[napi]` wrappers and the REST/gRPC client construction.

use napi::Unknown;
use napi_derive::napi;

use nqql_common as common;

// SAFETY (CodeQL rust/access-invalid-pointer false positive): `#[napi]` on
// these structs expands via napi-derive into unsafe Node-API glue that
// dereferences raw `napi_env`/`napi_value` pointers. That generated code is
// audited upstream (napi-rs) and upholds safety via the Node-API runtime
// (valid env, main-thread calls, `catch_unwind` on every entry point). This
// crate contains no manual `unsafe` blocks or raw-pointer dereferences.
// codeql[rust/access-invalid-pointer]: napi-derive generated FFI, not manual unsafe.
// (Inline suppression is forward-compat: Rust has no AlertSuppression.ql yet,
// see github/codeql#21637; current alerts are dismissed as false positives.)

/// Thin `#[napi]` handle over the parsed QQL AST (logic lives in nqql-common).
#[napi]
#[derive(Clone)]
pub struct Stmt {
    pub(crate) inner: qql_core::ast::Stmt,
    pub(crate) bound: bool,
}

#[napi]
impl Stmt {
    /// Parse a QQL string into a Stmt handle (mirrors `qql-wasm`'s
    /// `new Stmt(query)` — see the filter-injection guide).
    #[napi(constructor, catch_unwind)]
    pub fn new(input: String) -> napi::Result<Self> {
        Ok(Stmt {
            inner: common::stmt_parse(&input).map_err(common::to_napi_err)?,
            bound: false,
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
    pub fn to_object(&self) -> napi::Result<common::jsoutput::BigIntSafeJson> {
        // BigInt-safe: snowflake u64 IDs cross as `BigInt`, safe ints as
        // `Number` — never a rounded f64 (see `nqql-common/src/jsoutput.rs`).
        serde_json::to_value(&self.inner)
            .map(common::jsoutput::BigIntSafeJson)
            .map_err(common::serde_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn to_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner).map_err(common::serde_napi_err)
    }

    /// QQL `SHARD` routing key (request-level). Prefer the clause in QQL.
    ///
    /// Reads back `string` for keyword keys, `bigint` for numeric keys, and
    /// `null` when unset (placeholders also read as `null` — bind first).
    /// The setter accepts `string | number | bigint | null`: numbers must be
    /// exact non-negative integers (larger keys need `BigInt`).
    #[napi(getter, catch_unwind)]
    pub fn shard_key(
        &self,
    ) -> Option<napi::bindgen_prelude::Either<String, napi::bindgen_prelude::BigInt>> {
        use napi::bindgen_prelude::{BigInt, Either};
        match self.inner.shard_key() {
            None => None,
            Some(qql_core::ast::ShardKey::Keyword(s)) => Some(Either::A(s.clone())),
            Some(qql_core::ast::ShardKey::Number(n)) => Some(Either::B(BigInt {
                sign_bit: false,
                words: vec![*n],
            })),
            // Unbound placeholders have no host value yet; bind first.
            Some(_) => None,
        }
    }

    #[napi(
        setter,
        catch_unwind,
        ts_type = "string | number | bigint | null | undefined"
    )]
    pub fn set_shard_key(&mut self, key: Option<Unknown<'_>>) -> napi::Result<()> {
        let key = match key {
            None => None,
            Some(value) => {
                common::jsparams::unknown_opt_to_shard_key(value).map_err(common::to_napi_err)?
            }
        };
        if !self.inner.set_shard_key(key) {
            return Err(napi::Error::from_reason(
                "cannot set shardKey on statement type that does not support sharding (e.g. DDL statements)",
            ));
        }
        Ok(())
    }

    /// Whether parameters have already been bound into this statement.
    #[napi(getter, catch_unwind)]
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// Bind parameters into this statement and return a new bound Stmt.
    /// Vector params accept plain arrays as well as `Float32Array` /
    /// `Float64Array` (one memcpy, no per-element walk).
    #[napi(catch_unwind)]
    pub fn bind(&self, params: Unknown<'_>) -> napi::Result<Self> {
        let params = common::jsparams::unknown_opt_to_value(params).map_err(common::to_napi_err)?;
        let binds_now = params.is_some();
        if binds_now && self.bound {
            return Err(common::to_napi_err(common::already_bound_error()));
        }
        let inner =
            common::stmt_bind_value(&self.inner, params.as_ref()).map_err(common::to_napi_err)?;
        Ok(Stmt {
            inner,
            bound: self.bound || binds_now,
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

    /// Tree-formatted plan explanation for this statement (mirrors the free
    /// `explainStmt` and `qql-wasm`'s `Stmt.explain`).
    #[napi(catch_unwind)]
    pub fn explain(&self) -> String {
        common::explain_stmt(&self.inner)
    }

    /// Compile this Stmt AST directly into its transport route without re-parsing.
    /// Optionally accepts `params` to bind before compiling.
    #[napi(catch_unwind)]
    pub fn compile_route(
        &self,
        params: Unknown<'_>,
    ) -> napi::Result<common::jsoutput::BigIntSafeJson> {
        let params = common::jsparams::unknown_opt_to_value(params).map_err(common::to_napi_err)?;
        let binds_now = params.is_some();
        if binds_now && self.bound {
            return Err(common::to_napi_err(common::already_bound_error()));
        }
        common::stmt_compile_route_value(&self.inner, params.as_ref())
            .map(common::jsoutput::BigIntSafeJson)
            .map_err(common::to_napi_err)
    }
}

#[napi(catch_unwind)]
pub fn parse_all(input: String) -> napi::Result<Vec<Stmt>> {
    Ok(common::parse_all(&input)
        .map_err(common::to_napi_err)?
        .into_iter()
        .map(|inner| Stmt {
            inner,
            bound: false,
        })
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
) -> napi::Result<common::jsoutput::BigIntSafeJson> {
    common::inject_filter(&query, &field, &op, value)
        .map(common::jsoutput::BigIntSafeJson)
        .map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn tokenize(input: String) -> napi::Result<common::jsoutput::BigIntSafeJson> {
    common::tokenize(&input)
        .map(common::jsoutput::BigIntSafeJson)
        .map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn compile(
    input: String,
    params: Unknown<'_>,
) -> napi::Result<common::jsoutput::BigIntSafeJson> {
    let params = common::jsparams::unknown_opt_to_value(params).map_err(common::to_napi_err)?;
    common::compile_query_value(&input, params.as_ref())
        .map(common::jsoutput::BigIntSafeJson)
        .map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn explain(query: String) -> napi::Result<String> {
    common::explain(&query).map_err(common::to_napi_err)
}

#[napi(catch_unwind)]
pub fn explain_stmt(stmt: &Stmt) -> napi::Result<String> {
    Ok(common::explain_stmt(&stmt.inner))
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
    params: Unknown<'_>,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let truncate = options
        .as_ref()
        .and_then(|o| {
            o.get("truncateVectors")
                .or_else(|| o.get("truncate_vectors"))
        })
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let params = common::jsparams::unknown_opt_to_value(params).map_err(common::to_napi_err)?;
    match params {
        Some(p) => {
            let plan =
                qql_core::params_json::plan_value_params(&p, 1).map_err(common::to_napi_err)?;
            qql_core::params_json::bind_str_with_values(
                &query,
                qql_core::params_json::param_value_for(&plan, 0),
                truncate,
            )
            .map_err(common::to_napi_err)
        }
        None => Ok(query),
    }
}

/// Read an optional embedder value by camelCase/snake_case keys. Missing or
/// `null` keeps the default (`None`); a present-but-wrong-typed value fails
/// closed with `QQL-VALIDATION-CONFIG` instead of silently keeping it.
fn bm25_opt<T>(
    emb: &serde_json::Value,
    camel: &str,
    snake: &str,
    expected: &str,
    convert: impl Fn(&serde_json::Value) -> Option<T>,
) -> napi::Result<Option<T>> {
    let key = if emb.get(camel).is_some() {
        camel
    } else {
        snake
    };
    let Some(value) = emb.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    convert(value).map(Some).ok_or_else(|| {
        common::to_napi_err(qql::QqlError::validation(
            "QQL-VALIDATION-CONFIG",
            format!("embedder.{key} must be {expected}"),
            None,
        ))
    })
}

fn create_js_executor(options: Option<serde_json::Value>) -> napi::Result<qql::executor::Executor> {
    let opts = options.unwrap_or_else(|| serde_json::json!({}));
    let url_str = opts
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("http://localhost:6333");
    let api_key = opts
        .get("apiKey")
        .or_else(|| opts.get("api_key"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let grpc = opts
        .get("useGrpc")
        .or_else(|| opts.get("use_grpc"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // Qdrant 1.19 read affinity: `X-Qdrant-Route-Affinity` header (REST) /
    // `x-qdrant-route-affinity` metadata (gRPC). Empty strings are unset.
    let route_affinity = opts
        .get("routeAffinity")
        .or_else(|| opts.get("route_affinity"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .filter(|s| !s.is_empty());

    let mut config = qql::config::QqlConfig {
        url: url_str.to_string(),
        secret: api_key.clone(),
        ..Default::default()
    };

    if let Some(emb) = opts.get("embedder")
        && let Some(ep) = emb.get("endpoint").and_then(|v| v.as_str())
    {
        config.embedding_endpoint = Some(ep.to_string());
        config.embedding_api_key = emb
            .get("apiKey")
            .or_else(|| emb.get("api_key"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.embedding_model = emb.get("model").and_then(|v| v.as_str()).map(String::from);
        config.embedding_dimension =
            emb.get("dimension").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        config.multi_embedding_endpoint = emb
            .get("multiEndpoint")
            .or_else(|| emb.get("multi_endpoint"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.multi_embedding_api_key = emb
            .get("multiApiKey")
            .or_else(|| emb.get("multi_api_key"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.multi_embedding_model = emb
            .get("multiModel")
            .or_else(|| emb.get("multi_model"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.multi_embedding_dimension = emb
            .get("multiDimension")
            .or_else(|| emb.get("multi_dimension"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        config.image_embedding_endpoint = emb
            .get("imageEndpoint")
            .or_else(|| emb.get("image_endpoint"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.image_embedding_api_key = emb
            .get("imageApiKey")
            .or_else(|| emb.get("image_api_key"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.image_embedding_model = emb
            .get("imageModel")
            .or_else(|| emb.get("image_model"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.image_embedding_dimension = emb
            .get("imageDimension")
            .or_else(|| emb.get("image_dimension"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        config.rerank_endpoint = emb
            .get("rerankEndpoint")
            .or_else(|| emb.get("rerank_endpoint"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.rerank_api_key = emb
            .get("rerankApiKey")
            .or_else(|| emb.get("rerank_api_key"))
            .and_then(|v| v.as_str())
            .map(String::from);
        config.rerank_model = emb
            .get("rerankModel")
            .or_else(|| emb.get("rerank_model"))
            .and_then(|v| v.as_str())
            .map(String::from);
        // Client-side BM25 document parameters for the local sparse path.
        // Present-but-wrong-typed values fail closed here with
        // `QQL-VALIDATION-CONFIG`; only missing/`null` keep defaults (a
        // silent `None` would never reach the native validator).
        config.bm25_k1 = bm25_opt(emb, "bm25K1", "bm25_k1", "a number", |v| v.as_f64())?;
        config.bm25_b = bm25_opt(emb, "bm25B", "bm25_b", "a number", |v| v.as_f64())?;
        config.bm25_avg_len = bm25_opt(emb, "bm25AvgLen", "bm25_avg_len", "a number", |v| {
            v.as_f64()
        })?;
        config.bm25_language = bm25_opt(emb, "bm25Language", "bm25_language", "a string", |v| {
            v.as_str().map(String::from)
        })?;
        config.bm25_tokenizer =
            bm25_opt(emb, "bm25Tokenizer", "bm25_tokenizer", "a string", |v| {
                v.as_str().map(String::from)
            })?;
        config.bm25_lowercase =
            bm25_opt(emb, "bm25Lowercase", "bm25_lowercase", "a boolean", |v| {
                v.as_bool()
            })?;
        config.bm25_ascii_folding = bm25_opt(
            emb,
            "bm25AsciiFolding",
            "bm25_ascii_folding",
            "a boolean",
            |v| v.as_bool(),
        )?;
        config.bm25_stopwords = bm25_opt(
            emb,
            "bm25Stopwords",
            "bm25_stopwords",
            "an array of strings",
            |v| {
                v.as_array().and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(String::from))
                        .collect::<Option<Vec<_>>>()
                })
            },
        )?;
        config.bm25_stopwords_languages = bm25_opt(
            emb,
            "bm25StopwordsLanguages",
            "bm25_stopwords_languages",
            "an array of strings",
            |v| {
                v.as_array().and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(String::from))
                        .collect::<Option<Vec<_>>>()
                })
            },
        )?;
        config.bm25_stemmer = bm25_opt(emb, "bm25Stemmer", "bm25_stemmer", "a string", |v| {
            v.as_str().map(String::from)
        })?;
        config.bm25_min_token_len = bm25_opt(
            emb,
            "bm25MinTokenLen",
            "bm25_min_token_len",
            "a non-negative integer",
            |v| v.as_u64().and_then(|n| usize::try_from(n).ok()),
        )?;
        config.bm25_max_token_len = bm25_opt(
            emb,
            "bm25MaxTokenLen",
            "bm25_max_token_len",
            "a non-negative integer",
            |v| v.as_u64().and_then(|n| usize::try_from(n).ok()),
        )?;
    }

    let client: Box<dyn qql::client::QdrantOps> = if grpc {
        #[cfg(feature = "grpc")]
        {
            // tonic's `connect_lazy` captures the tokio reactor at
            // construction — build the channel inside napi's global tokio
            // runtime (which drives every async #[napi] call), otherwise
            // construction panics on the JS thread with "there is no reactor
            // running".
            let handle =
                napi::bindgen_prelude::block_on(async { tokio::runtime::Handle::current() });
            let mut grpc = {
                let _enter = handle.enter();
                qql::grpc::GrpcQdrant::from_url(url_str, api_key).map_err(common::to_napi_err)?
            };
            if let Some(affinity) = route_affinity.as_deref() {
                grpc = grpc.with_route_affinity(affinity);
            }
            Box::new(grpc)
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err(napi::Error::from_reason(
                "gRPC feature not enabled in this build",
            ));
        }
    } else {
        let mut rest = qql::rest::RestQdrant::new(url_str.to_string(), api_key);
        if let Some(affinity) = route_affinity.as_deref() {
            rest = rest.with_route_affinity(affinity);
        }
        Box::new(rest)
    };

    let embedder = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let http_emb =
                qql::embedder::HttpEmbedder::try_with_options(qql::embedder::HttpEmbedderOptions {
                    endpoint: endpoint.clone(),
                    api_key: config.embedding_api_key.clone().unwrap_or_default(),
                    model: config.embedding_model.clone().unwrap_or_default(),
                    dimension: config.embedding_dimension,
                    multi_endpoint: config.multi_embedding_endpoint.clone(),
                    multi_api_key: config.multi_embedding_api_key.clone(),
                    multi_model: config.multi_embedding_model.clone(),
                    multi_dimension: config.multi_embedding_dimension,
                    image_endpoint: config.image_embedding_endpoint.clone(),
                    image_api_key: config.image_embedding_api_key.clone(),
                    image_model: config.image_embedding_model.clone(),
                    image_dimension: config.image_embedding_dimension,
                    rerank_endpoint: config.rerank_endpoint.clone(),
                    rerank_api_key: config.rerank_api_key.clone(),
                    rerank_model: config.rerank_model.clone(),
                    bm25_k1: config.bm25_k1,
                    bm25_b: config.bm25_b,
                    bm25_avg_len: config.bm25_avg_len,
                    bm25_language: config.bm25_language.clone(),
                    bm25_tokenizer: config.bm25_tokenizer.clone(),
                    bm25_lowercase: config.bm25_lowercase,
                    bm25_ascii_folding: config.bm25_ascii_folding,
                    bm25_stopwords: config.bm25_stopwords.clone(),
                    bm25_stopwords_languages: config.bm25_stopwords_languages.clone(),
                    bm25_stemmer: config.bm25_stemmer.clone(),
                    bm25_min_token_len: config.bm25_min_token_len,
                    bm25_max_token_len: config.bm25_max_token_len,
                })
                .map_err(common::to_napi_err)?;
            Some(std::sync::Arc::new(http_emb) as std::sync::Arc<dyn qql::embedder::Embedder>)
        } else {
            None
        }
    } else {
        None
    };

    let exec = qql::executor::Executor::with_embedder(client, Some(config), embedder);

    Ok(exec)
}

/// Node-API client handle (REST/gRPC executor + route affinity).
/// See SAFETY note at the top of this file for the `#[napi]` false positive.
#[napi(js_name = "Client")]
pub struct JsClient {
    inner: qql::executor::Executor,
    /// Normalized `X-Qdrant-Route-Affinity` value (REST header / gRPC metadata).
    route_affinity: Option<String>,
}

#[napi]
impl JsClient {
    /// Constructor required by napi-rs class registry.  Always throws —
    /// use `new Client(options)`.
    #[napi(constructor, catch_unwind)]
    pub fn new(options: Option<serde_json::Value>) -> napi::Result<Self> {
        let route_affinity = options
            .as_ref()
            .and_then(|o| o.get("routeAffinity"))
            .or_else(|| options.as_ref().and_then(|o| o.get("route_affinity")))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
        let exec = create_js_executor(options)?;
        Ok(JsClient {
            inner: exec,
            route_affinity,
        })
    }

    /// Read affinity key pinning reads to a stable replica
    /// (`X-Qdrant-Route-Affinity`, Qdrant 1.19+). Set via
    /// `new Client({ routeAffinity })`.
    #[napi(getter, catch_unwind)]
    pub fn route_affinity(&self) -> Option<String> {
        self.route_affinity.clone()
    }

    /// Execute a QQL query string, a Stmt, or an array of either.
    /// Multi-statement strings (semicolons) and arrays are auto-batched.
    /// Returns the ExecutionReport as a live JS object for the JavaScript
    /// wrapper: safe integers cross as `Number`, snowflake u64 IDs as
    /// `BigInt` (a JSON string would round them through `JSON.parse`).
    #[napi(
        catch_unwind,
        ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue', params?: Record<string, any> | any[] }"
    )]
    pub async fn execute(
        &self,
        query: serde_json::Value,
        options: Option<common::execute::ExecOptionsInput>,
    ) -> napi::Result<common::jsoutput::BigIntSafeJson> {
        let (on_error, params) = common::execute::typed_dispatch_inputs(options.as_ref());
        let report = common::execute::execute_dispatch_typed(&self.inner, query, on_error, params)
            .await
            .map_err(common::to_napi_err)?;
        serde_json::to_value(&report)
            .map(common::jsoutput::BigIntSafeJson)
            .map_err(common::serde_napi_err)
    }

    #[napi(catch_unwind)]
    pub fn explain(&self, query: String) -> napi::Result<String> {
        qql::executor::Executor::explain(&query).map_err(common::to_napi_err)
    }

    /// Analyze a single query string or Stmt: static plan plus measured
    /// execution (phase timings, server time, hardware/inference usage).
    /// Returns the AnalyzeReport as a live JS object (BigInt-safe like
    /// `execute`). Batches fail closed.
    #[napi(
        catch_unwind,
        ts_args_type = "query: string | Stmt, options?: { onError?: 'stop' | 'continue', params?: Record<string, any> | any[] }"
    )]
    pub async fn explain_analyze(
        &self,
        query: serde_json::Value,
        options: Option<common::execute::ExecOptionsInput>,
    ) -> napi::Result<common::jsoutput::BigIntSafeJson> {
        let (on_error, params) = common::execute::typed_dispatch_inputs(options.as_ref());
        let report =
            common::execute::explain_analyze_dispatch_typed(&self.inner, query, on_error, params)
                .await
                .map_err(common::to_napi_err)?;
        serde_json::to_value(&report)
            .map(common::jsoutput::BigIntSafeJson)
            .map_err(common::serde_napi_err)
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
        params: Unknown<'_>,
    ) -> napi::Result<common::jsoutput::BigIntSafeJson> {
        let params = common::jsparams::unknown_opt_to_value(params).map_err(common::to_napi_err)?;
        common::compile_query_value(&query, params.as_ref())
            .map(common::jsoutput::BigIntSafeJson)
            .map_err(common::to_napi_err)
    }

    /// Bulk ingest: `rows` is an array of point objects
    /// (`{id, vector, …payload}`) spliced through the `:rows` point-splice
    /// path in `batchSize` chunks (default 100). Vectors accept plain arrays
    /// and the flat `{data, dim}` multivector form. The JS wrapper normalizes
    /// `Float32Array`/`Float64Array` and integer typed arrays to plain arrays
    /// before this serde boundary, so direct native callers should pass plain
    /// arrays (or bind typed arrays on the sync `Stmt.bind` surface instead,
    /// then execute the bound statement). Returns the ExecutionReport as a
    /// live JS object (BigInt-safe like `execute`).
    #[napi(
        catch_unwind,
        ts_args_type = "collection: string, rows: Record<string, any>[], options?: { onError?: 'stop' | 'continue', batchSize?: number }"
    )]
    pub async fn upsert_many(
        &self,
        collection: String,
        rows: serde_json::Value,
        options: Option<serde_json::Value>,
    ) -> napi::Result<common::jsoutput::BigIntSafeJson> {
        let rows = common::value_from_json(rows).map_err(common::to_napi_err)?;
        let batch_size =
            common::execute::batch_size_from(options.as_ref()).map_err(common::to_napi_err)?;
        let on_error = common::execute::on_error_from(options.as_ref());
        let report = common::execute::upsert_many_dispatch(
            &self.inner,
            collection,
            rows,
            batch_size,
            on_error,
        )
        .await
        .map_err(common::to_napi_err)?;
        serde_json::to_value(&report)
            .map(common::jsoutput::BigIntSafeJson)
            .map_err(common::serde_napi_err)
    }

    /// Close the client and release underlying connections.
    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        self.inner.close().await.map_err(common::to_napi_err)
    }
}

/// Execute a pre-parsed Stmt directly via a new temporary client.
#[napi(catch_unwind, ts_args_type = "stmt: Stmt, options?: object")]
pub async fn execute_stmt(
    stmt: &Stmt,
    options: Option<common::execute::ExecOptionsInput>,
) -> napi::Result<common::jsoutput::BigIntSafeJson> {
    let raw = options.as_ref().map(|o| o.raw.clone());
    let params = options.as_ref().and_then(|o| o.params.as_ref());
    let client = JsClient::new(raw)?;
    let resp = common::execute::run_then_close(&client.inner, async {
        let mut inner = stmt.inner.clone();
        if let Some(p) = params {
            let plan = qql_core::params_json::plan_value_params(p, 1)?;
            qql_core::params_json::bind_stmt_with_values(
                &mut inner,
                qql_core::params_json::param_value_for(&plan, 0),
            )?;
        }
        client.inner.execute_node(inner).await
    })
    .await
    .map_err(common::to_napi_err)?;
    let report = qql::executor::ExecutionReport::single(resp);
    serde_json::to_value(&report)
        .map(common::jsoutput::BigIntSafeJson)
        .map_err(common::serde_napi_err)
}

#[napi(
    catch_unwind,
    ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue', params?: Record<string, any> | any[] }"
)]
pub async fn execute(
    query: serde_json::Value,
    options: Option<common::execute::ExecOptionsInput>,
) -> napi::Result<common::jsoutput::BigIntSafeJson> {
    let raw = options.as_ref().map(|o| o.raw.clone());
    let (on_error, params) = common::execute::typed_dispatch_inputs(options.as_ref());
    let client = JsClient::new(raw)?;
    let report = common::execute::run_then_close(
        &client.inner,
        common::execute::execute_dispatch_typed(&client.inner, query, on_error, params),
    )
    .await
    .map_err(common::to_napi_err)?;
    serde_json::to_value(&report)
        .map(common::jsoutput::BigIntSafeJson)
        .map_err(common::serde_napi_err)
}
