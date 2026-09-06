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
    // Full frontend gate: parse + plan — same contract as execution and the
    // language conformance suite.
    qql_plan::parse_and_plan(&input).is_ok()
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
pub fn compile_query(
    input: String,
    params: Option<serde_json::Value>,
) -> napi::Result<serde_json::Value> {
    let mut stmt = Parser::parse(&input).map_err(to_napi_err)?;
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

    if let Some(emb) = opts.get("embedder") {
        if let Some(ep) = emb.get("endpoint").and_then(|v| v.as_str()) {
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
    }

    let client: Box<dyn qql::client::QdrantOps> = if grpc {
        #[cfg(feature = "grpc")]
        {
            let mut grpc =
                qql::grpc::GrpcQdrant::from_url(url_str, api_key).map_err(to_napi_err)?;
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
                .map_err(to_napi_err)?;
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

        let get_batch_param = |idx: usize| -> Option<&serde_json::Value> {
            if let Some(serde_json::Value::Array(param_list)) = params {
                if !param_list.is_empty() && (param_list[0].is_object() || param_list[0].is_array())
                {
                    return param_list.get(idx);
                }
            }
            params
        };

        let report = match &query {
            serde_json::Value::String(s) => {
                let has_semicolon = s.contains(';');
                let is_batch_params = match params {
                    Some(serde_json::Value::Array(arr)) => {
                        !arr.is_empty() && (arr[0].is_object() || arr[0].is_array())
                    }
                    _ => false,
                };

                if has_semicolon && is_batch_params {
                    let mut stmts = Parser::parse_all(s).map_err(to_napi_err)?;
                    for (i, stmt) in stmts.iter_mut().enumerate() {
                        if let Some(p) = get_batch_param(i) {
                            bind_stmt_json(stmt, p).map_err(to_napi_err)?;
                        }
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
                } else {
                    let effective_query = if let Some(p) = params {
                        bind_json_params(s, p, false)?
                    } else {
                        s.clone()
                    };
                    self.inner
                        .execute(&effective_query, on_error)
                        .await
                        .map_err(to_napi_err)?
                }
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    qql::executor::ExecutionReport::empty()
                } else if arr[0].is_string() {
                    let strs: Vec<String> = arr
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let s = v.as_str().ok_or_else(|| {
                                napi::Error::from_reason("batch items must be strings")
                            })?;
                            if let Some(p) = get_batch_param(i) {
                                bind_json_params(s, p, false)
                            } else {
                                Ok(s.to_string())
                            }
                        })
                        .collect::<napi::Result<_>>()?;
                    let str_refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
                    self.inner
                        .execute_batch(&str_refs, on_error)
                        .await
                        .map_err(to_napi_err)?
                } else {
                    let mut stmts: Vec<ast::Stmt> = arr
                        .iter()
                        .map(|v| {
                            serde_json::from_value(v.clone())
                                .map_err(|e| napi::Error::from_reason(format!("invalid Stmt: {e}")))
                        })
                        .collect::<napi::Result<_>>()?;
                    for (i, stmt) in stmts.iter_mut().enumerate() {
                        if let Some(p) = get_batch_param(i) {
                            bind_stmt_json(stmt, p).map_err(to_napi_err)?;
                        }
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
    pub fn compile(
        &self,
        query: String,
        params: Option<serde_json::Value>,
    ) -> napi::Result<serde_json::Value> {
        crate::compile_query(query, params)
    }

    /// Close the client and release underlying connections.
    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        self.inner.close().await.map_err(to_napi_err)
    }
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
        .and_then(|o| {
            o.get("truncateVectors")
                .or_else(|| o.get("truncate_vectors"))
        })
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    bind_json_params(&query, &params, truncate)
}

#[napi(catch_unwind)]
pub fn explain(query: String) -> napi::Result<String> {
    qql_core::explain::explain(&query).map_err(to_napi_err)
}

#[napi(catch_unwind)]
pub fn explain_stmt(stmt: &Stmt) -> napi::Result<String> {
    Ok(qql_core::explain::explain_node(&stmt.inner))
}

/// Execute a pre-parsed Stmt directly via a new temporary client.
#[napi(catch_unwind, ts_args_type = "stmt: Stmt, options?: object")]
pub async fn execute_stmt(stmt: &Stmt, options: Option<serde_json::Value>) -> napi::Result<String> {
    let client = JsClient::new(options)?;
    let resp = client
        .inner
        .execute_node(stmt.inner.clone())
        .await
        .map_err(to_napi_err)?;
    let report = qql::executor::ExecutionReport::single(resp);
    serde_json::to_string(&report).map_err(serde_napi_err)
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
    client.execute(query, options).await
}
