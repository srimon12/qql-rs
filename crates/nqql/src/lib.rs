//! nqql — native Node.js bindings for the QQL parser and runtime.
//!
//! The parser/parameter/execution logic lives in `nqql-common`, shared with
//! `nqql-edge` so the two SDKs cannot drift; this crate keeps only the
//! `#[napi]` wrappers and the REST/gRPC client construction.

use napi_derive::napi;

use nqql_common as common;

#[napi]
#[derive(Clone)]
pub struct Stmt {
    inner: qql_core::ast::Stmt,
}

#[napi]
impl Stmt {
    /// Parse a QQL string into a Stmt handle (mirrors `qql-wasm`'s
    /// `new Stmt(query)` — see the filter-injection guide).
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

    /// QQL `SHARD '…'` routing key (request-level). Prefer `SHARD` in the query;
    /// set after parse only when the host resolves the key dynamically.
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
    params: Option<serde_json::Value>,
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

    /// Close the client and release underlying connections.
    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        self.inner.close().await.map_err(common::to_napi_err)
    }
}

/// Execute a pre-parsed Stmt directly via a new temporary client.
#[napi(catch_unwind, ts_args_type = "stmt: Stmt, options?: object")]
pub async fn execute_stmt(stmt: &Stmt, options: Option<serde_json::Value>) -> napi::Result<String> {
    let client = JsClient::new(options.clone())?;
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

#[napi(
    catch_unwind,
    ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue', params?: Record<string, any> | any[] }"
)]
pub async fn execute(
    query: serde_json::Value,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let client = JsClient::new(options.clone())?;
    let report = common::execute::execute_dispatch(&client.inner, query, options.as_ref())
        .await
        .map_err(common::to_napi_err)?;
    client.inner.close().await.map_err(common::to_napi_err)?;
    serde_json::to_string(&report).map_err(common::serde_napi_err)
}
