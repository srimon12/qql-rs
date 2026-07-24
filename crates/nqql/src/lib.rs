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
    #[napi]
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

    #[napi]
    pub fn to_object(&self) -> napi::Result<serde_json::Value> {
        serde_json::to_value(&self.inner).map_err(serde_napi_err)
    }

    #[napi]
    pub fn to_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner).map_err(serde_napi_err)
    }

    /// Get or set the shard key on QUERY, COUNT, SCROLL, UPSERT, and DELETE
    /// statements.  Returns `null` (setter is no-op) for other statement types.
    #[napi(getter)]
    pub fn shard_key(&self) -> Option<String> {
        match &self.inner {
            ast::Stmt::Query(q) => q.shard_key.clone(),
            ast::Stmt::Count(c) => c.shard_key.clone(),
            ast::Stmt::Scroll(s) => s.shard_key.clone(),
            ast::Stmt::Upsert(u) => u.shard_key.clone(),
            ast::Stmt::Delete(d) => d.shard_key.clone(),
            _ => None,
        }
    }

    #[napi(setter)]
    pub fn set_shard_key(&mut self, key: Option<String>) {
        let key = key.filter(|k| !k.is_empty());
        match &mut self.inner {
            ast::Stmt::Query(q) => q.shard_key = key,
            ast::Stmt::Count(c) => c.shard_key = key,
            ast::Stmt::Scroll(s) => s.shard_key = key,
            ast::Stmt::Upsert(u) => u.shard_key = key,
            ast::Stmt::Delete(d) => d.shard_key = key,
            _ => {}
        }
    }
}

#[napi]
pub fn parse_all(input: String) -> napi::Result<Vec<Stmt>> {
    let stmts = Parser::parse_all(&input).map_err(to_napi_err)?;
    Ok(stmts.into_iter().map(|s| Stmt { inner: s }).collect())
}

#[napi]
pub fn is_valid(input: String) -> bool {
    Parser::parse_all(&input).is_ok()
}

#[napi]
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

#[napi]
pub fn tokenize(input: String) -> napi::Result<serde_json::Value> {
    #[derive(serde::Serialize)]
    struct TokenView<'a> {
        kind: &'a str,
        text: &'a str,
        pos: usize,
    }

    let lexer = Lexer::new(&input);
    let mut tokens = Vec::new();
    for token_result in lexer {
        let token =
            token_result.map_err(|e| napi::Error::new(napi::Status::InvalidArg, e.to_string()))?;
        tokens.push(TokenView {
            kind: token.kind.as_str(),
            text: token.text,
            pos: token.span.start,
        });
    }
    serde_json::to_value(&tokens).map_err(|e| {
        napi::Error::new(
            napi::Status::GenericFailure,
            format!("failed to serialize tokens: {}", e),
        )
    })
}

#[napi]
pub fn compile_query(input: String) -> napi::Result<serde_json::Value> {
    let stmt = Parser::parse(&input).map_err(to_napi_err)?;
    let route = routing::route(&stmt);
    let output = serde_json::json!({
        "stmt_type": match &route.body {
            Some(qql_plan::routing::RequestBody::Query(_)) => "query",
            Some(qql_plan::routing::RequestBody::QueryGroups(_)) => "query_groups",
            Some(qql_plan::routing::RequestBody::Points(_)) => "points",
            Some(qql_plan::routing::RequestBody::Scroll(_)) => "scroll",
            Some(qql_plan::routing::RequestBody::Upsert(_)) => "upsert",
            Some(qql_plan::routing::RequestBody::Delete(_)) => "delete",
            Some(qql_plan::routing::RequestBody::UpdateVector(_)) => "update_vector",
            Some(qql_plan::routing::RequestBody::UpdatePayload(_)) => "update_payload",
            Some(qql_plan::routing::RequestBody::ClearPayload(_)) => "clear_payload",
            Some(qql_plan::routing::RequestBody::DeleteVector(_)) => "delete_vector",
            Some(qql_plan::routing::RequestBody::Count(_)) => "count",
            Some(qql_plan::routing::RequestBody::CreateShardKey(_)) => "create_shard_key",
            Some(qql_plan::routing::RequestBody::DropShardKey(_)) => "drop_shard_key",
            Some(qql_plan::routing::RequestBody::CreateCollection(_)) => "create_collection",
            Some(qql_plan::routing::RequestBody::UpdateCollection(_)) => "update_collection",
            Some(qql_plan::routing::RequestBody::CreateIndex(_)) => "create_index",
            None => match route.method {
                qql_plan::types::Method::Get if route.path == "/collections" => "show_collections",
                qql_plan::types::Method::Get => "show_collection",
                qql_plan::types::Method::Delete => "drop_collection",
                _ => "unknown",
            },
        },
        "method": route.method.as_str(),
        "path": route.path,
        "payload": route.body_json().unwrap_or(serde_json::Value::Null),
    });
    Ok(output)
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
        }
    }

    let client: Box<dyn qql::client::QdrantOps> = if grpc {
        #[cfg(feature = "grpc")]
        {
            Box::new(qql::grpc::GrpcQdrant::from_url(url_str, api_key).map_err(to_napi_err)?)
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err(napi::Error::from_reason(
                "gRPC feature not enabled in this build",
            ));
        }
    } else {
        Box::new(qql::rest::RestQdrant::new(url_str.to_string(), api_key))
    };

    let embedder = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let api_key = config.embedding_api_key.clone().unwrap_or_default();
            let model = config.embedding_model.clone().unwrap_or_default();
            let dim = config.embedding_dimension;
            let http_emb = qql::embedder::HttpEmbedder::new(endpoint.clone(), api_key, model, dim)
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
}

#[napi]
impl JsClient {
    #[napi(constructor)]
    pub fn new(options: Option<serde_json::Value>) -> napi::Result<Self> {
        let exec = create_js_executor(options)?;
        Ok(JsClient { inner: exec })
    }

    /// Execute a QQL query string, a Stmt, or an array of either.
    /// Multi-statement strings (semicolons) and arrays are auto-batched.
    /// Returns a stable ExecutionReport JSON string for the JavaScript wrapper
    /// to deserialize into an object.
    #[napi(
        ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue' }"
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

        let report = match &query {
            serde_json::Value::String(s) => {
                self.inner.execute(s, on_error).await.map_err(to_napi_err)?
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    qql::executor::ExecutionReport::empty()
                } else if arr[0].is_string() {
                    let strs: Vec<&str> = arr
                        .iter()
                        .map(|v| {
                            v.as_str().ok_or_else(|| {
                                napi::Error::from_reason("batch items must be strings")
                            })
                        })
                        .collect::<napi::Result<_>>()?;
                    self.inner
                        .execute_batch(&strs, on_error)
                        .await
                        .map_err(to_napi_err)?
                } else {
                    let stmts: Vec<ast::Stmt> = arr
                        .iter()
                        .map(|v| {
                            serde_json::from_value(v.clone())
                                .map_err(|e| napi::Error::from_reason(format!("invalid Stmt: {e}")))
                        })
                        .collect::<napi::Result<_>>()?;
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
                let s: ast::Stmt = serde_json::from_value(query)
                    .map_err(|e| napi::Error::from_reason(format!("invalid Stmt: {e}")))?;
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

    #[napi]
    pub fn explain(&self, query: String) -> napi::Result<String> {
        qql::executor::Executor::explain(&query).map_err(to_napi_err)
    }

    #[napi]
    pub fn explain_stmt(&self, stmt: &Stmt) -> napi::Result<String> {
        qql::executor::Executor::explain_node(&stmt.inner).map_err(to_napi_err)
    }

    /// Compile a QQL query to its transport route (non-executing).
    #[napi]
    pub fn compile(&self, query: String) -> napi::Result<serde_json::Value> {
        crate::compile_query(query)
    }
}

#[napi]
pub fn explain(query: String) -> napi::Result<String> {
    qql_core::explain::explain(&query).map_err(to_napi_err)
}

#[napi]
pub fn explain_stmt(stmt: &Stmt) -> napi::Result<String> {
    Ok(qql_core::explain::explain_node(&stmt.inner))
}

/// Execute a pre-parsed Stmt directly via a new temporary client.
#[napi(ts_args_type = "stmt: Stmt, options?: object")]
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
    ts_args_type = "query: string | Stmt | string[] | Stmt[], options?: { onError?: 'stop' | 'continue' }"
)]
pub async fn execute(
    query: serde_json::Value,
    options: Option<serde_json::Value>,
) -> napi::Result<String> {
    let client = JsClient::new(options.clone())?;
    client.execute(query, options).await
}
