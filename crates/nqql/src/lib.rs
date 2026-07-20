use napi_derive::napi;
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::offline;
use qql_core::parser::Parser;

#[napi(js_name = "Stmt")]
#[derive(Clone)]
pub struct NapiStmt {
    inner: qql_core::ast::Stmt,
}

#[napi]
impl NapiStmt {
    #[napi]
    pub fn inject_filter(
        &mut self,
        field: String,
        op: String,
        value: serde_json::Value,
    ) -> napi::Result<()> {
        let val = Value::from_json(value)
            .ok_or_else(|| napi::Error::from_reason("invalid value JSON"))?;
        ast::inject_filter(&mut self.inner, &field, &op, &val);
        Ok(())
    }

    #[napi]
    pub fn to_object(&self) -> napi::Result<serde_json::Value> {
        serde_json::to_value(&self.inner).map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn to_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner).map_err(|e| napi::Error::from_reason(e.to_string()))
    }
}

#[napi]
pub fn parse(input: String) -> napi::Result<NapiStmt> {
    let stmt = Parser::parse(&input).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(NapiStmt { inner: stmt })
}

#[napi]
pub fn parse_all(input: String) -> napi::Result<Vec<NapiStmt>> {
    let stmts = Parser::parse_all(&input).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(stmts.into_iter().map(|s| NapiStmt { inner: s }).collect())
}

#[napi]
pub fn parse_batch(queries: Vec<String>) -> napi::Result<Vec<NapiStmt>> {
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        let stmt = Parser::parse(&q).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        results.push(NapiStmt { inner: stmt });
    }
    Ok(results)
}

#[napi]
pub fn parse_json(input: String) -> napi::Result<String> {
    match Parser::parse(&input) {
        Ok(stmt) => {
            serde_json::to_string(&stmt).map_err(|e| napi::Error::from_reason(e.to_string()))
        }
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
}

#[napi]
pub fn parse_batch_json(queries: Vec<String>) -> napi::Result<String> {
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        let stmt = Parser::parse(&q).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        results.push(stmt);
    }
    serde_json::to_string(&results).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
pub fn is_valid(input: String) -> bool {
    Parser::try_parse(&input).is_ok()
}

#[napi]
pub fn inject_filter(
    query: String,
    field: String,
    op: String,
    value: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    let value =
        Value::from_json(value).ok_or_else(|| napi::Error::from_reason("invalid value JSON"))?;
    let mut stmt = Parser::parse(&query).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    ast::inject_filter(&mut stmt, &field, &op, &value);
    serde_json::to_value(&stmt).map_err(|e| napi::Error::from_reason(e.to_string()))
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
            pos: token.pos,
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
    let compiled = offline::compile(&input).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    serde_json::to_value(&compiled).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi(js_name = "HttpEmbedder")]
#[derive(Clone)]
pub struct JsHttpEmbedder {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub dimension: u32,
}

#[napi]
impl JsHttpEmbedder {
    #[napi(constructor)]
    pub fn new(options: serde_json::Value) -> napi::Result<Self> {
        let ep = options
            .get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let key = options
            .get("apiKey")
            .or_else(|| options.get("api_key"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let model = options
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let dim = options
            .get("dimension")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        Ok(JsHttpEmbedder {
            endpoint: ep,
            api_key: key,
            model,
            dimension: dim,
        })
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
            Box::new(
                qql::grpc::GrpcQdrant::from_url(url_str, api_key)
                    .map_err(|e| napi::Error::from_reason(e.to_string()))?,
            )
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err(napi::Error::from_reason(
                "gRPC feature not enabled in this build",
            ));
        }
    } else {
        Box::new(
            qql::rest::RestQdrant::new(url_str.to_string(), api_key)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?,
        )
    };

    let embedder = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let api_key = config.embedding_api_key.clone().unwrap_or_default();
            let model = config.embedding_model.clone().unwrap_or_default();
            let dim = config.embedding_dimension;
            let http_emb = qql::embedder::HttpEmbedder::new(endpoint.clone(), api_key, model, dim)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
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

    #[napi]
    pub async fn execute(&self, query: String) -> napi::Result<serde_json::Value> {
        let res = self
            .inner
            .execute(&query)
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        serde_json::to_value(&res).map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub async fn execute_stmt(&self, stmt: &NapiStmt) -> napi::Result<serde_json::Value> {
        let res = self
            .inner
            .execute_node(stmt.inner.clone())
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        serde_json::to_value(&res).map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub async fn execute_json(&self, query: String) -> napi::Result<String> {
        let res = self
            .inner
            .execute(&query)
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        serde_json::to_string(&res).map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub async fn execute_stmt_json(&self, stmt: &NapiStmt) -> napi::Result<String> {
        let res = self
            .inner
            .execute_node(stmt.inner.clone())
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        serde_json::to_string(&res).map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn explain(&self, query: String) -> napi::Result<String> {
        qql::executor::Executor::explain(&query)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn explain_stmt(&self, stmt: &NapiStmt) -> napi::Result<String> {
        qql::executor::Executor::explain_node(&stmt.inner)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }
}

#[napi]
pub async fn execute(
    query: String,
    options: Option<serde_json::Value>,
) -> napi::Result<serde_json::Value> {
    let client = JsClient::new(options)?;
    client.execute(query).await
}

#[napi]
pub async fn execute_stmt(
    stmt: &NapiStmt,
    options: Option<serde_json::Value>,
) -> napi::Result<serde_json::Value> {
    let client = JsClient::new(options)?;
    client.execute_stmt(stmt).await
}

#[napi]
pub fn explain(query: String) -> napi::Result<String> {
    qql::executor::Executor::explain(&query).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
pub fn explain_stmt(stmt: &NapiStmt) -> napi::Result<String> {
    qql::executor::Executor::explain_node(&stmt.inner)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}
