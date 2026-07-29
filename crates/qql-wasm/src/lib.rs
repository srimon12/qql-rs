#[cfg(all(feature = "client", target_arch = "wasm32"))]
use async_trait::async_trait;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
use gloo_net::http::Request;
use qql_core::ast::{self, ComparisonOp, Value};
#[cfg(all(feature = "client", target_arch = "wasm32"))]
use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
use qql_embed::{Embedder, SparseVector};
use qql_plan::routing;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
use serde_json::json;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const EXECUTION_TYPES: &str = r#"
export interface ExecuteOptions {
  onError?: "stop" | "continue";
}

export interface ExecResponse {
  ok: boolean;
  operation: string;
  message: string;
  data: unknown | null;
}

export interface ExecutionReport {
  ok: boolean;
  results: ExecResponse[];
  succeeded: number;
  failed: number;
}

export interface Token {
  kind: string;
  text: string;
  pos: number;
  end: number;
  len: number;
}

export interface CompiledRoute {
  stmt_type: string;
  method: string;
  path: string;
  payload: unknown | null;
}

export interface AnalysisError {
  code: string;
  message: string;
  start: number | null;
  end: number | null;
}

export interface AnalysisResult {
  valid: boolean;
  statements_count: number;
  tokens: Token[];
  ast: unknown[] | null;
  route: CompiledRoute | null;
  routes: CompiledRoute[];
  explain: string | null;
  error: AnalysisError | null;
}
"#;

// ── Core: parsing ────────────────────────────────────────────────

#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod report {
    /// Internal execution report matching the qql-runtime contract.
    /// Used so callers can access typed `succeeded`/`failed`/`results`
    /// fields without chasing `serde_json::Value` keys.
    #[derive(serde::Serialize)]
    pub struct WasmReport {
        pub ok: bool,
        pub results: Vec<serde_json::Value>,
        pub succeeded: usize,
        pub failed: usize,
    }

    impl WasmReport {
        pub fn from_results(results: Vec<serde_json::Value>) -> Self {
            let succeeded = results
                .iter()
                .filter(|r| r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
                .count();
            let failed = results.len() - succeeded;
            Self {
                ok: failed == 0,
                results,
                succeeded,
                failed,
            }
        }

        pub fn single(resp: serde_json::Value) -> Self {
            let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            Self {
                ok,
                results: vec![resp],
                succeeded: if ok { 1 } else { 0 },
                failed: if ok { 0 } else { 1 },
            }
        }

        pub fn empty() -> Self {
            Self {
                ok: true,
                results: Vec::new(),
                succeeded: 0,
                failed: 0,
            }
        }
    }

    /// Build an ExecResponse-compatible JSON value.
    pub fn exec_response(
        ok: bool,
        operation: &str,
        message: &str,
        data: Option<serde_json::Value>,
    ) -> serde_json::Value {
        serde_json::json!({
            "ok": ok,
            "operation": operation,
            "message": message,
            "data": data.unwrap_or(serde_json::Value::Null),
        })
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
use report::{exec_response, WasmReport};

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WasmOnError {
    Stop,
    Continue,
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[derive(Clone, PartialEq, Eq)]
enum WasmBatchKey {
    Query(String),
    Mutation(String),
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn wasm_statement_batch_key(stmt: &qql_core::ast::Stmt) -> Option<WasmBatchKey> {
    use qql_core::ast::{QueryCollection, QueryExpr, Stmt};

    match stmt {
        Stmt::Query(query)
            if query.group.is_none() && !matches!(query.expression, QueryExpr::Points { .. }) =>
        {
            match &query.collection {
                QueryCollection::Explicit(collection) => {
                    Some(WasmBatchKey::Query(collection.clone()))
                }
                QueryCollection::Inherited => None,
            }
        }
        Stmt::Upsert(stmt) => Some(WasmBatchKey::Mutation(stmt.collection.clone())),
        Stmt::Delete(stmt) => Some(WasmBatchKey::Mutation(stmt.collection.clone())),
        Stmt::UpdatePayload(stmt) => Some(WasmBatchKey::Mutation(stmt.collection.clone())),
        Stmt::ClearPayload(stmt) => Some(WasmBatchKey::Mutation(stmt.collection.clone())),
        Stmt::UpdateVector(stmt) => Some(WasmBatchKey::Mutation(stmt.collection.clone())),
        Stmt::DeleteVector(stmt) => Some(WasmBatchKey::Mutation(stmt.collection.clone())),
        _ => None,
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn wasm_planned_batch_key(operation: &qql_plan::PlannedOperation) -> Option<WasmBatchKey> {
    use qql_plan::{BatchFamily, PlannedOperation};

    match operation.batch_family() {
        BatchFamily::Query => match operation {
            PlannedOperation::Query { collection, .. } => {
                Some(WasmBatchKey::Query(collection.clone()))
            }
            _ => None,
        },
        BatchFamily::Mutation => operation
            .collection()
            .map(|collection| WasmBatchKey::Mutation(collection.to_owned())),
        BatchFamily::Single => None,
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn parse_on_error(options: Option<JsValue>) -> Result<WasmOnError, JsValue> {
    let Some(options) = options else {
        return Ok(WasmOnError::Stop);
    };
    if options.is_null() || options.is_undefined() {
        return Ok(WasmOnError::Stop);
    }
    if !options.is_object() {
        return Err(JsValue::from_str("options must be an object"));
    }
    let value = js_sys::Reflect::get(&options, &JsValue::from_str("onError"))?;
    if value.is_undefined() {
        return Ok(WasmOnError::Stop);
    }
    match value.as_string().as_deref() {
        Some("stop") => Ok(WasmOnError::Stop),
        Some("continue") => Ok(WasmOnError::Continue),
        _ => Err(JsValue::from_str(
            "options.onError must be 'stop' or 'continue'",
        )),
    }
}

thread_local! {
    static SCRATCH_BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::with_capacity(8192));
}

/// Safely copies slice into a JS-owned Uint8Array, avoiding WASM memory reentrancy/relocation issues.
fn safe_owned_uint8_array(bytes: &[u8]) -> js_sys::Uint8Array {
    let view = unsafe { js_sys::Uint8Array::view(bytes) };
    js_sys::Uint8Array::new(&view)
}

#[wasm_bindgen(unchecked_return_type = "unknown[]")]
pub fn parse(input: &str) -> Result<JsValue, JsValue> {
    // Always parse as a script — returns a list even for single statements.
    let stmts = Parser::parse_all(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&stmts)
}

#[wasm_bindgen(js_name = isValid)]
pub fn is_valid(input: &str) -> bool {
    Parser::parse_all(input).is_ok()
}

#[wasm_bindgen]
pub fn inject_filter(
    query: &str,
    field: &str,
    op: &str,
    value: JsValue,
) -> Result<JsValue, JsValue> {
    let serde_value: serde_json::Value = serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsValue::from_str(&format!("invalid value: {}", e)))?;
    let val = Value::from_json(serde_value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let cmp = parse_comparison_op(op)?;
    let mut stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    ast::inject_filter(&mut stmt, field, cmp, val)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&stmt)
}

/// Inject a shard key for multi-tenant routing (QUERY/SCROLL/COUNT/UPSERT/DELETE + CTEs).
#[wasm_bindgen(js_name = injectShardKey)]
pub fn inject_shard_key(query: &str, shard_key: &str) -> Result<JsValue, JsValue> {
    let mut stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    ast::inject_shard_key(&mut stmt, shard_key).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&stmt)
}

fn parse_comparison_op(op: &str) -> Result<ComparisonOp, JsValue> {
    match op {
        "=" | "==" | "eq" => Ok(ComparisonOp::Eq),
        ">" | "gt" => Ok(ComparisonOp::Gt),
        ">=" | "gte" => Ok(ComparisonOp::Gte),
        "<" | "lt" => Ok(ComparisonOp::Lt),
        "<=" | "lte" => Ok(ComparisonOp::Lte),
        "!=" | "neq" | "<>" => Err(JsValue::from_str(
            "inject_filter does not support '!='; inject equality and wrap with NOT, or rewrite the query",
        )),
        other => Err(JsValue::from_str(&format!(
            "unsupported comparison operator '{other}' (use =, >, >=, <, <=)"
        ))),
    }
}

// ── Stmt class ─────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct Stmt {
    inner: qql_core::ast::Stmt,
}

#[wasm_bindgen]
impl Stmt {
    /// Parse a QQL string into a Stmt object for programmatic manipulation.
    #[wasm_bindgen(constructor)]
    pub fn new(input: &str) -> Result<Stmt, JsValue> {
        let inner = Parser::parse(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Stmt { inner })
    }

    /// Inject a WHERE filter into this statement's AST (mutates in place).
    #[wasm_bindgen(js_name = injectFilter)]
    pub fn inject_filter(&mut self, field: &str, op: &str, value: JsValue) -> Result<(), JsValue> {
        let serde_value: serde_json::Value = serde_wasm_bindgen::from_value(value)
            .map_err(|e| JsValue::from_str(&format!("invalid value: {}", e)))?;
        let val = Value::from_json(serde_value).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let cmp = parse_comparison_op(op)?;
        ast::inject_filter(&mut self.inner, field, cmp, val)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(())
    }

    /// Multi-tenant shard routing: set shard key on this statement (+ nested CTEs).
    #[wasm_bindgen(js_name = injectShardKey)]
    pub fn inject_shard_key(&mut self, shard_key: &str) -> Result<(), JsValue> {
        ast::inject_shard_key(&mut self.inner, shard_key)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get or set the shard key on statements that support custom sharding.
    #[wasm_bindgen(getter, js_name = shardKey)]
    pub fn shard_key(&self) -> Option<String> {
        match &self.inner {
            ast::Stmt::Query(q) => q.shard_key.clone(),
            ast::Stmt::Count(c) => c.shard_key.clone(),
            ast::Stmt::Scroll(s) => s.shard_key.clone(),
            ast::Stmt::Upsert(u) => u.shard_key.clone(),
            ast::Stmt::Delete(d) => d.shard_key.clone(),
            ast::Stmt::ClearPayload(c) => c.shard_key.clone(),
            ast::Stmt::DeleteVector(d) => d.shard_key.clone(),
            ast::Stmt::UpdateVector(u) => u.shard_key.clone(),
            ast::Stmt::UpdatePayload(u) => u.shard_key.clone(),
            _ => None,
        }
    }

    #[wasm_bindgen(setter, js_name = shardKey)]
    pub fn set_shard_key(&mut self, key: Option<String>) {
        let key = key.filter(|k| !k.is_empty());
        match &mut self.inner {
            ast::Stmt::Query(q) => q.shard_key = key,
            ast::Stmt::Count(c) => c.shard_key = key,
            ast::Stmt::Scroll(s) => s.shard_key = key,
            ast::Stmt::Upsert(u) => u.shard_key = key,
            ast::Stmt::Delete(d) => d.shard_key = key,
            ast::Stmt::ClearPayload(c) => c.shard_key = key,
            ast::Stmt::DeleteVector(d) => d.shard_key = key,
            ast::Stmt::UpdateVector(u) => u.shard_key = key,
            ast::Stmt::UpdatePayload(u) => u.shard_key = key,
            _ => {}
        }
    }

    /// Serialise the AST to a JSON string.
    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.inner).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Serialise the AST to a JS object.
    #[wasm_bindgen(js_name = toObject)]
    pub fn to_object(&self) -> Result<JsValue, JsValue> {
        to_js_value(&self.inner)
    }

    /// Compile this Stmt AST directly into a Qdrant REST route object.
    #[wasm_bindgen(js_name = compileRoute, unchecked_return_type = "CompiledRoute")]
    pub fn compile_route(&self) -> Result<JsValue, JsValue> {
        let (stmt_type, route) = routing::compile_statement(&self.inner)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let output = serde_json::json!({
            "stmt_type": stmt_type,
            "method": route.method.as_str(),
            "path": route.path,
            "payload": route.body_json().unwrap_or(serde_json::Value::Null),
        });
        to_js_value(&output)
    }

    /// Compile this Stmt AST into a JS-owned Uint8Array byte buffer.
    #[wasm_bindgen(js_name = compileRouteBytes)]
    pub fn compile_route_bytes(&self) -> Result<js_sys::Uint8Array, JsValue> {
        let (stmt_type, route) = routing::compile_statement(&self.inner)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let output = serde_json::json!({
            "stmt_type": stmt_type,
            "method": route.method.as_str(),
            "path": route.path,
            "payload": route.body_json().unwrap_or(serde_json::Value::Null),
        });
        SCRATCH_BUF.with(|cell| {
            let mut buf = cell.borrow_mut();
            buf.clear();
            serde_json::to_writer(&mut *buf, &output)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(safe_owned_uint8_array(&buf))
        })
    }
}

// ── Core: tokenize ────────────────────────────────────────────────

#[wasm_bindgen]
pub fn tokenize(input: &str) -> Result<Vec<JsValue>, JsValue> {
    let lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(|e| JsValue::from_str(&e.to_string()))?;
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("kind"),
            &JsValue::from_str(token.kind.as_str()),
        )
        .unwrap();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("text"),
            &JsValue::from_str(token.text),
        )
        .unwrap();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("pos"),
            &JsValue::from_f64(token.span.start as f64),
        )
        .unwrap();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("end"),
            &JsValue::from_f64(token.span.end as f64),
        )
        .unwrap();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("len"),
            &JsValue::from_f64(token.span.end.saturating_sub(token.span.start) as f64),
        )
        .unwrap();
        tokens.push(JsValue::from(obj));
    }
    Ok(tokens)
}

// ── Core: unified analyze ─────────────────────────────────────────

fn build_analyze_value(input: &str) -> serde_json::Value {
    let mut tokens = Vec::new();
    let lexer = Lexer::new(input);
    for t in lexer.flatten() {
        tokens.push(serde_json::json!({
            "kind": t.kind.as_str(),
            "text": t.text,
            "pos": t.span.start,
            "end": t.span.end,
            "len": t.span.end.saturating_sub(t.span.start),
        }));
    }

    let stmts_res = Parser::parse_all(input);
    match stmts_res {
        Ok(stmts) => {
            let ast_val = serde_json::to_value(&stmts).unwrap_or(serde_json::Value::Null);
            let routes_val: Vec<_> = stmts
                .iter()
                .filter_map(|s| {
                    let (stmt_type, r) = routing::compile_statement(s).ok()?;
                    Some(serde_json::json!({
                        "stmt_type": stmt_type,
                        "method": r.method.as_str(),
                        "path": r.path,
                        "payload": r.body_json().unwrap_or(serde_json::Value::Null),
                    }))
                })
                .collect();
            let route_val = routes_val
                .first()
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            let explain_val = qql_core::explain::explain_nodes(&stmts);

            serde_json::json!({
                "valid": true,
                "statements_count": stmts.len(),
                "tokens": tokens,
                "ast": ast_val,
                "route": route_val,
                "routes": routes_val,
                "explain": explain_val,
                "error": serde_json::Value::Null,
            })
        }
        Err(err) => {
            let err_json = serde_json::json!({
                "code": err.code.as_ref(),
                "message": err.message.as_ref(),
                "start": err.span.map(|s| s.start),
                "end": err.span.map(|s| s.end),
            });

            serde_json::json!({
                "valid": false,
                "statements_count": 0,
                "tokens": tokens,
                "ast": serde_json::Value::Null,
                "route": serde_json::Value::Null,
                "routes": [],
                "explain": serde_json::Value::Null,
                "error": err_json,
            })
        }
    }
}

fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    // The JSON-compatible serializer emits plain JavaScript objects/arrays,
    // including for serde_json::Value. Keep serialized JSON/bytes behind
    // explicit APIs such as compileBytes, not the default JS-facing contract.
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    val.serialize(&serializer)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(unchecked_return_type = "AnalysisResult")]
pub fn analyze(input: &str) -> Result<JsValue, JsValue> {
    let val = build_analyze_value(input);
    to_js_value(&val)
}

// ── Core: compile & explain ───────────────────────────────────────

fn build_compile_output(query: &str) -> Result<serde_json::Value, JsValue> {
    let stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let (stmt_type, route) =
        routing::compile_statement(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(serde_json::json!({
        "stmt_type": stmt_type,
        "method": route.method.as_str(),
        "path": route.path,
        "payload": route.body_json().unwrap_or(serde_json::Value::Null),
    }))
}

/// Compile one QQL statement into a JavaScript route object.
#[wasm_bindgen(unchecked_return_type = "CompiledRoute")]
pub fn compile(query: &str) -> Result<JsValue, JsValue> {
    let output = build_compile_output(query)?;
    to_js_value(&output)
}

/// Compiles QQL query into a safe, JS-owned Uint8Array byte buffer.
#[wasm_bindgen(js_name = compileBytes)]
pub fn compile_bytes(query: &str) -> Result<js_sys::Uint8Array, JsValue> {
    let output = build_compile_output(query)?;
    SCRATCH_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.clear();
        serde_json::to_writer(&mut *buf, &output).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(safe_owned_uint8_array(&buf))
    })
}

#[wasm_bindgen]
pub fn explain(query: &str) -> Result<String, JsValue> {
    qql_core::explain::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = explainBytes)]
pub fn explain_bytes(query: &str) -> Result<js_sys::Uint8Array, JsValue> {
    let exp_str =
        qql_core::explain::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(safe_owned_uint8_array(exp_str.as_bytes()))
}

// ── Client: browser fetch-based execute with embedding ────────────

#[cfg(all(feature = "client", target_arch = "wasm32"))]
enum EmbedMode {
    None,
    /// JS function: `async (texts: string[]) => number[][]` (already batched).
    Js(js_sys::Function),
    /// OpenAI-compatible HTTP: POST `{"model", "input": string[]}`.
    /// User must supply the full endpoint (OpenAI, Ollama `/v1/embeddings`, etc.).
    Http,
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[wasm_bindgen]
pub struct Client {
    url: String,
    api_key: Option<String>,

    embed_mode: EmbedMode,
    embed_endpoint: String,
    embed_api_key: Option<String>,
    embed_model: String,
    embed_dim: u32,
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    pub fn new(url: Option<String>, api_key: Option<String>) -> Client {
        Client {
            url: url.unwrap_or_else(|| "http://localhost:6333".to_string()),
            api_key,
            embed_mode: EmbedMode::None,
            embed_endpoint: String::new(),
            embed_api_key: None,
            embed_model: String::new(),
            embed_dim: 0,
        }
    }

    // ── Embedder configuration ──────────────────────────────────

    /// Set a JS embedder: `async (texts: string[]) => number[][]`.
    /// Called with the full batch — do not loop one-by-one inside the callback
    /// if your model supports batching (Transformers.js pipeline, etc.).
    #[wasm_bindgen(js_name = setEmbedder)]
    pub fn set_embedder(
        &mut self,
        #[wasm_bindgen(
            unchecked_param_type = "(texts: string[]) => Promise<number[][]> | number[][]"
        )]
        fn_: js_sys::Function,
    ) {
        self.embed_mode = EmbedMode::Js(fn_);
    }

    /// OpenAI-compatible HTTP embedder. **No default URL** — pass the full
    /// embeddings endpoint you intend to use, e.g.:
    /// - `https://api.openai.com/v1/embeddings`
    /// - `http://localhost:11434/v1/embeddings` (Ollama)
    /// - any provider that accepts `{"model","input":[...]}` and returns
    ///   `{"data":[{"embedding":[...],"index":0},...]}`.
    ///
    /// Always sends the whole text batch in one request (`input` as array).
    #[wasm_bindgen(js_name = setHttpEmbedder)]
    pub fn set_http_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        if endpoint.trim().is_empty() {
            return Err(JsValue::from_str(
                "setHttpEmbedder: endpoint is required (no default URL)",
            ));
        }
        if model.trim().is_empty() {
            return Err(JsValue::from_str("setHttpEmbedder: model is required"));
        }
        if dimension == 0 {
            return Err(JsValue::from_str(
                "setHttpEmbedder: dimension must be positive",
            ));
        }
        self.embed_mode = EmbedMode::Http;
        self.embed_endpoint = endpoint;
        self.embed_api_key = api_key;
        self.embed_model = model;
        self.embed_dim = dimension;
        Ok(())
    }

    /// Alias for [`set_http_embedder`] — same OpenAI-compatible protocol.
    #[wasm_bindgen(js_name = setRemoteEmbedder)]
    pub fn set_remote_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        self.set_http_embedder(endpoint, model, dimension, api_key)
    }

    /// Check whether any embedder is configured.
    #[wasm_bindgen(js_name = hasEmbedder)]
    pub fn has_embedder(&self) -> bool {
        !matches!(self.embed_mode, EmbedMode::None)
    }

    fn request(&self, method: &str, path: &str) -> gloo_net::http::RequestBuilder {
        let mut rb = match method {
            "GET" => Request::get(&format!("{}{}", self.url, path)),
            "POST" => Request::post(&format!("{}{}", self.url, path)),
            "PUT" => Request::put(&format!("{}{}", self.url, path)),
            "PATCH" => Request::patch(&format!("{}{}", self.url, path)),
            "DELETE" => Request::delete(&format!("{}{}", self.url, path)),
            _ => Request::get(&format!("{}{}", self.url, path)),
        };
        if let Some(ref key) = self.api_key {
            rb = rb.header("api-key", key);
        }
        rb = rb.header("Content-Type", "application/json");
        rb
    }

    /// Embed a batch of texts. Returns vectors in the same order.
    #[cfg(target_arch = "wasm32")]
    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, JsValue> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        match &self.embed_mode {
            EmbedMode::Js(fn_) => {
                let array = js_sys::Array::new();
                for t in &texts {
                    array.push(&JsValue::from_str(t));
                }
                let returned = fn_
                    .call1(&JsValue::NULL, &array)
                    .map_err(|e| JsValue::from_str(&format!("embedder call failed: {:?}", e)))?;

                let result = if returned.is_instance_of::<js_sys::Promise>() {
                    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(returned))
                        .await
                        .map_err(|e| JsValue::from_str(&format!("embedder rejected: {:?}", e)))?
                } else {
                    returned
                };

                let rows: Vec<Vec<f32>> = serde_wasm_bindgen::from_value(result).map_err(|e| {
                    JsValue::from_str(&format!("embedder returned invalid vectors: {}", e))
                })?;

                if rows.len() != texts.len() {
                    return Err(JsValue::from_str(&format!(
                        "embedder returned {} vectors, expected {}",
                        rows.len(),
                        texts.len()
                    )));
                }
                Ok(rows)
            }

            EmbedMode::Http => {
                // Single HTTP request: input = full array (OpenAI/Ollama/Cohere compat).
                let body = json!({ "model": self.embed_model, "input": texts });
                let resp = self.post_with_auth(&self.embed_endpoint, &body).await?;
                Self::parse_openai_batch_response(&resp, texts.len(), self.embed_dim)
            }

            EmbedMode::None => Ok(Vec::new()),
        }
    }

    /// POST JSON with Bearer auth to embedding endpoint.
    #[cfg(target_arch = "wasm32")]
    async fn post_with_auth(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, JsValue> {
        let body_str =
            serde_json::to_string(body).map_err(|e| JsValue::from_str(&e.to_string()))?;

        let mut rb = Request::post(url).header("Content-Type", "application/json");
        if let Some(ref key) = self.embed_api_key {
            rb = rb.header("Authorization", &format!("Bearer {}", key));
        }

        let resp = rb
            .body(body_str)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
            .send()
            .await
            .map_err(|e| JsValue::from_str(&format!("embedding API error: {}", e)))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        if status >= 400 {
            return Err(JsValue::from_str(&format!(
                "embedding API returned {}: {}",
                status, text
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| JsValue::from_str(&format!("invalid embedding API response: {}", e)))
    }

    /// Parse OpenAI-compatible batch response:
    /// `{"data":[{"embedding":[...],"index":0}, ...]}` — reorders by `index` when present.
    #[cfg(target_arch = "wasm32")]
    fn parse_openai_batch_response(
        resp: &serde_json::Value,
        expected: usize,
        expected_dim: u32,
    ) -> Result<Vec<Vec<f32>>, JsValue> {
        let data = resp["data"]
            .as_array()
            .ok_or_else(|| JsValue::from_str("embedding response missing 'data' array"))?;

        let mut slots: Vec<Option<Vec<f32>>> = vec![None; expected];
        for (fallback_i, item) in data.iter().enumerate() {
            let emb = item["embedding"]
                .as_array()
                .ok_or_else(|| JsValue::from_str("item missing 'embedding' array"))?;
            if expected_dim > 0 && emb.len() != expected_dim as usize {
                return Err(JsValue::from_str(&format!(
                    "embedding dimension mismatch: got {}, expected {}",
                    emb.len(),
                    expected_dim
                )));
            }
            let vec: Vec<f32> = emb
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            let idx = item["index"].as_u64().unwrap_or(fallback_i as u64) as usize;
            if idx >= expected {
                return Err(JsValue::from_str(&format!(
                    "embedding index {idx} out of range (batch size {expected})"
                )));
            }
            if slots[idx].is_some() {
                return Err(JsValue::from_str(&format!(
                    "duplicate embedding index {idx}"
                )));
            }
            slots[idx] = Some(vec);
        }

        slots
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                v.ok_or_else(|| JsValue::from_str(&format!("missing embedding at index {i}")))
            })
            .collect()
    }

    /// Shared AST resolve via `qql-embed` (batched dense + local sparse).
    #[cfg(target_arch = "wasm32")]
    async fn resolve_stmt_embeddings(&self, stmt: &mut qql_core::ast::Stmt) -> Result<(), JsValue> {
        if !self.has_embedder() {
            return Ok(());
        }
        qql_embed::resolve_embeddings(stmt, self)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn resolve_stmt_embeddings(
        &self,
        _stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        Ok(())
    }

    /// Parse, compile, embed if needed, and POST to Qdrant's REST API.
    ///
    /// Accepts a string (single statement or semicolon-delimited script) or
    /// a `string[]`. Always returns a stable `ExecutionReport` object:
    /// `{ "ok": bool, "results": [...], "succeeded": N, "failed": M }`.
    #[wasm_bindgen(unchecked_return_type = "ExecutionReport")]
    pub async fn execute(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string | string[]")] query: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "ExecuteOptions")] options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let on_error = parse_on_error(options)?;
        if js_sys::Array::is_array(&query) {
            let arr = js_sys::Array::from(&query);
            let len = arr.length() as usize;
            let mut all_results: Vec<serde_json::Value> = Vec::new();
            let mut succeeded = 0usize;
            let mut failed = 0usize;
            for i in 0..len {
                let item = arr.get(i as u32);
                let s = item.as_string().ok_or_else(|| {
                    JsValue::from_str(&format!(
                        "array item at index {} must be a string, got {:?}",
                        i,
                        item.js_typeof()
                    ))
                })?;
                match self.execute_script(&s, on_error).await {
                    Ok(report) => {
                        succeeded += report.succeeded;
                        failed += report.failed;
                        all_results.extend(report.results);
                    }
                    Err(e) => {
                        if on_error == WasmOnError::Stop {
                            return Err(e);
                        }
                        failed += 1;
                        all_results.push(exec_response(
                            false,
                            "ERROR",
                            &e.as_string().unwrap_or_default(),
                            None,
                        ));
                    }
                }
            }
            let report = WasmReport {
                ok: failed == 0,
                results: all_results,
                succeeded,
                failed,
            };
            return to_js_value(&report);
        }

        if let Some(s) = query.as_string() {
            let report = self.execute_script(&s, on_error).await?;
            return to_js_value(&report);
        }

        Err(JsValue::from_str("query must be a string or string[]"))
    }

    /// Execute a pre-parsed Stmt object.  Injects embeddings for UPSERT
    /// if an embedder is configured.
    #[wasm_bindgen(js_name = executeStmt, unchecked_return_type = "ExecutionReport")]
    pub async fn execute_stmt(&self, stmt: &Stmt) -> Result<JsValue, JsValue> {
        let val = self.execute_stmt_inner(&stmt.inner).await?;
        let report = WasmReport::single(val);
        to_js_value(&report)
    }

    /// Execute one or more statements with order-preserving smart batching.
    /// Returns a JSON value shaped like ExecutionReport.
    async fn execute_script(
        &self,
        query: &str,
        on_error: WasmOnError,
    ) -> Result<WasmReport, JsValue> {
        let stmts = match Parser::parse_all(query) {
            Ok(stmts) => stmts,
            Err(error) if on_error == WasmOnError::Stop => {
                return Err(JsValue::from_str(&error.to_string()));
            }
            Err(error) => {
                return Ok(WasmReport::single(exec_response(
                    false,
                    "PARSE",
                    &error.to_string(),
                    None,
                )));
            }
        };
        if stmts.is_empty() {
            return Ok(WasmReport::empty());
        }

        let mut results: Vec<serde_json::Value> = Vec::with_capacity(stmts.len());
        let mut pending = Vec::new();
        let mut pending_key: Option<WasmBatchKey> = None;

        for stmt in stmts {
            let statement_key = wasm_statement_batch_key(&stmt);
            if !pending.is_empty() && statement_key != pending_key {
                self.flush_planned_group(&mut pending, on_error, &mut results)
                    .await?;
                pending_key = None;
            }

            let planned = match self.prepare_operation(&stmt).await {
                Ok(planned) => planned,
                Err(error) => {
                    self.flush_planned_group(&mut pending, on_error, &mut results)
                        .await?;
                    pending_key = None;
                    if on_error == WasmOnError::Stop {
                        return Err(error);
                    }
                    results.push(exec_response(
                        false,
                        "PREPARE",
                        &error.as_string().unwrap_or_default(),
                        None,
                    ));
                    continue;
                }
            };

            let key = wasm_planned_batch_key(&planned);
            if key.is_none() {
                self.flush_planned_group(&mut pending, on_error, &mut results)
                    .await?;
                pending_key = None;
                self.dispatch_or_collect(planned, on_error, &mut results)
                    .await?;
                continue;
            }

            if !pending.is_empty() && key != pending_key {
                self.flush_planned_group(&mut pending, on_error, &mut results)
                    .await?;
            }
            pending_key = key;
            pending.push(planned);
        }

        self.flush_planned_group(&mut pending, on_error, &mut results)
            .await?;
        Ok(WasmReport::from_results(results))
    }

    async fn prepare_operation(
        &self,
        stmt: &qql_core::ast::Stmt,
    ) -> Result<qql_plan::PlannedOperation, JsValue> {
        let mut stmt = stmt.clone();
        // Schema-first: fill USING kinds from collection topology before
        // embedding so `USING sparse` embeds sparse, not dense-by-default.
        self.resolve_stmt_vector_kinds(&mut stmt).await?;
        self.resolve_stmt_embeddings(&mut stmt).await?;
        qql_plan::plan(&stmt).map_err(|error| JsValue::from_str(&error.to_string()))
    }

    /// Fetch collection topology and resolve `USING` vector kinds.
    #[cfg(target_arch = "wasm32")]
    async fn resolve_stmt_vector_kinds(
        &self,
        stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        let qql_core::ast::Stmt::Query(query) = stmt else {
            return Ok(());
        };
        let qql_core::ast::QueryCollection::Explicit(collection) = &query.collection else {
            return Ok(());
        };
        if !qql_embed::query_needs_kind_resolution(query) {
            return Ok(());
        }
        let collection = collection.clone();
        let topology = self.fetch_vector_topology(&collection).await?;
        qql_embed::resolve_query_vector_kinds(&collection, query, &topology)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn resolve_stmt_vector_kinds(
        &self,
        _stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    async fn fetch_vector_topology(
        &self,
        collection: &str,
    ) -> Result<qql_embed::TopologyNames, JsValue> {
        let path = format!("/collections/{collection}");
        let body = self.send_json("GET", &path, None).await?;
        let result = body
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Ok(vector_names_from_collection_result(&result))
    }

    async fn execute_planned_inner(
        &self,
        operation: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, JsValue> {
        let route = qql_plan::to_rest_route(operation);
        let result = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        Ok(wasm_success_response(operation, result))
    }

    async fn dispatch_or_collect(
        &self,
        operation: qql_plan::PlannedOperation,
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        match self.execute_planned_inner(&operation).await {
            Ok(response) => results.push(response),
            Err(error) if on_error == WasmOnError::Stop => return Err(error),
            Err(error) => results.push(exec_response(
                false,
                operation.operation_label(),
                &error.as_string().unwrap_or_default(),
                None,
            )),
        }
        Ok(())
    }

    async fn flush_planned_group(
        &self,
        pending: &mut Vec<qql_plan::PlannedOperation>,
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        use qql_plan::mutation::planned_to_update_operation;
        use qql_plan::{PlannedOperation, QueryBatchRequest, UpdateBatchRequest};

        if pending.is_empty() {
            return Ok(());
        }
        if pending.len() == 1 {
            let operation = pending.pop().expect("pending contains one operation");
            return self.dispatch_or_collect(operation, on_error, results).await;
        }

        let operations = core::mem::take(pending);
        match &operations[0] {
            PlannedOperation::Query { collection, .. } => {
                let collection = collection.clone();
                let searches = operations
                    .iter()
                    .map(|operation| match operation {
                        PlannedOperation::Query { request, .. } => Ok(request.clone()),
                        _ => Err(JsValue::from_str(
                            "QQL-BATCH-INVARIANT: query batch contained a non-query operation",
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let expected = searches.len();
                let batch = QueryBatchRequest { searches };
                let path = format!("/collections/{collection}/points/query/batch");
                let body = serde_json::to_value(&batch)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => {
                        let values = response
                            .get("result")
                            .and_then(serde_json::Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        if values.len() != expected {
                            let error = JsValue::from_str(&format!(
                                "QQL-BATCH-CARDINALITY: query batch returned {} results for {expected} operations",
                                values.len()
                            ));
                            self.collect_batch_error(
                                error,
                                &vec!["QUERY"; expected],
                                on_error,
                                results,
                            )?;
                        } else {
                            for value in values {
                                let hits = wasm_search_hits(&value);
                                let count = hits.as_array().map_or(0, Vec::len);
                                results.push(exec_response(
                                    true,
                                    "QUERY",
                                    &format!("Found {count} hits"),
                                    Some(hits),
                                ));
                            }
                        }
                    }
                    Err(error) => self.collect_batch_error(
                        error,
                        &vec!["QUERY"; expected],
                        on_error,
                        results,
                    )?,
                }
            }
            _ => {
                let mut updates = Vec::with_capacity(operations.len());
                let mut labels = Vec::with_capacity(operations.len());
                let mut collection = None;
                for operation in &operations {
                    let Some((current_collection, update)) = planned_to_update_operation(operation)
                    else {
                        return Err(JsValue::from_str(
                            "QQL-BATCH-INVARIANT: mutation batch contained a non-mutation operation",
                        ));
                    };
                    if collection
                        .as_ref()
                        .is_some_and(|collection| collection != &current_collection)
                    {
                        return Err(JsValue::from_str(
                            "QQL-BATCH-INVARIANT: mutation batch contained multiple collections",
                        ));
                    }
                    collection.get_or_insert(current_collection);
                    labels.push(update.operation_name());
                    updates.push(update);
                }
                let collection = collection.unwrap_or_default();
                let expected = updates.len();
                let batch = UpdateBatchRequest {
                    operations: updates,
                };
                let path = format!("/collections/{collection}/points/batch?wait=true");
                let body = serde_json::to_value(&batch)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => {
                        let values = response
                            .get("result")
                            .and_then(serde_json::Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        if values.len() != expected {
                            let error = JsValue::from_str(&format!(
                                "QQL-BATCH-CARDINALITY: update batch returned {} results for {expected} operations",
                                values.len()
                            ));
                            self.collect_batch_error(error, &labels, on_error, results)?;
                        } else {
                            for (value, label) in values.into_iter().zip(labels.iter()) {
                                results.push(exec_response(
                                    true,
                                    label,
                                    &format!("{label} ok (batched)"),
                                    Some(value),
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        self.collect_batch_error(error, &labels, on_error, results)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn collect_batch_error(
        &self,
        error: JsValue,
        labels: &[&str],
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        if on_error == WasmOnError::Stop {
            return Err(error);
        }
        let message = error.as_string().unwrap_or_default();
        results.extend(
            labels
                .iter()
                .map(|label| exec_response(false, label, &message, None)),
        );
        Ok(())
    }

    async fn execute_stmt_inner(
        &self,
        stmt: &qql_core::ast::Stmt,
    ) -> Result<serde_json::Value, JsValue> {
        let operation = self.prepare_operation(stmt).await?;
        self.execute_planned_inner(&operation).await
    }

    async fn send_json(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsValue> {
        let body_str = body
            .as_ref()
            .map(|b| serde_json::to_string(b).map_err(|e| JsValue::from_str(&e.to_string())))
            .transpose()?;

        let rb = self.request(method, path);
        let resp = if let Some(s) = body_str {
            rb.body(s)
                .map_err(|e| JsValue::from_str(&e.to_string()))?
                .send()
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?
        } else {
            rb.send()
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?
        };

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        if status >= 400 {
            return Err(JsValue::from_str(&format!(
                "Qdrant returned {}: {}",
                status, text
            )));
        }

        serde_json::from_str(&text).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Parse and compile one statement without executing it.
    #[wasm_bindgen(unchecked_return_type = "CompiledRoute")]
    pub fn compile(&self, query: &str) -> Result<JsValue, JsValue> {
        compile(query)
    }

    /// Parse and explain the query — no server needed.
    #[wasm_bindgen]
    pub fn explain(&self, query: &str) -> Result<String, JsValue> {
        qql_core::explain::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

/// Extract named dense/sparse/multivector names from a Qdrant collection `result` object.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn vector_names_from_collection_result(result: &serde_json::Value) -> qql_embed::TopologyNames {
    let params = result.get("config").and_then(|c| c.get("params"));
    let mut dense = Vec::new();
    let mut multivector = Vec::new();
    if let Some(vectors) = params
        .and_then(|p| p.get("vectors"))
        .and_then(|v| v.as_object())
    {
        if vectors.contains_key("size") && vectors.contains_key("distance") {
            // Unnamed default dense vector.
            dense.clear();
        } else {
            for (name, cfg) in vectors {
                if matches!(
                    name.as_str(),
                    "size"
                        | "distance"
                        | "hnsw_config"
                        | "quantization_config"
                        | "on_disk"
                        | "multivector_config"
                ) {
                    continue;
                }
                dense.push(name.clone());
                if cfg.get("multivector_config").is_some() {
                    multivector.push(name.clone());
                }
            }
            dense.sort();
            multivector.sort();
        }
    }
    let mut sparse = Vec::new();
    if let Some(map) = params
        .and_then(|p| p.get("sparse_vectors"))
        .and_then(|v| v.as_object())
    {
        sparse.extend(map.keys().cloned());
        sparse.sort();
    }
    qql_embed::TopologyNames {
        dense,
        sparse,
        multivector,
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn wasm_search_hits(result: &serde_json::Value) -> serde_json::Value {
    let points = result
        .get("result")
        .and_then(|value| value.get("points"))
        .and_then(serde_json::Value::as_array)
        .or_else(|| result.get("points").and_then(serde_json::Value::as_array))
        .or_else(|| result.get("result").and_then(serde_json::Value::as_array));

    serde_json::Value::Array(
        points
            .into_iter()
            .flatten()
            .map(|hit| {
                let id = hit
                    .get("id")
                    .map(|id| match id {
                        serde_json::Value::String(value) => value.clone(),
                        serde_json::Value::Number(value) => value.to_string(),
                        _ => id.to_string(),
                    })
                    .unwrap_or_default();
                let payload = hit
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let text = payload
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "id": id,
                    "score": hit.get("score").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                    "text": text,
                    "payload": payload,
                })
            })
            .collect(),
    )
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn wasm_success_response(
    operation: &qql_plan::PlannedOperation,
    result: serde_json::Value,
) -> serde_json::Value {
    use qql_plan::PlannedOperation;

    let label = operation.operation_label();
    let (message, data) = match operation {
        PlannedOperation::Query { .. }
        | PlannedOperation::Scroll { .. }
        | PlannedOperation::GetPoints { .. } => {
            let hits = wasm_search_hits(&result);
            let count = hits.as_array().map_or(0, Vec::len);
            (format!("Found {count} hits"), Some(hits))
        }
        PlannedOperation::QueryGroups { .. } => {
            let count = result
                .get("result")
                .and_then(|value| value.get("groups"))
                .or_else(|| result.get("groups"))
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            (format!("Found {count} group(s)"), Some(result))
        }
        PlannedOperation::Count { .. } => {
            let count = result
                .get("result")
                .and_then(|value| value.get("count"))
                .or_else(|| result.get("count"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            (format!("Count: {count}"), Some(result))
        }
        PlannedOperation::Upsert { request, .. } => (
            format!("Upserted {} point(s)", request.points.len()),
            Some(serde_json::json!({"count": request.points.len()})),
        ),
        PlannedOperation::ListShardKeys { .. } => ("Shard keys listed".to_string(), Some(result)),
        _ => (format!("{label} ok"), None),
    };
    exec_response(true, label, &message, data)
}

// ── WASM dense embed collect/apply (mirrors runtime batching) ─────

#[cfg(all(feature = "client", target_arch = "wasm32"))]
// ── qql-embed::Embedder adapter (shared resolve path) ─────────────
#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[async_trait(?Send)]
impl Embedder for Client {
    async fn embed_dense(&self, text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        let batch = self
            .embed_texts(vec![text.to_string()])
            .await
            .map_err(|e| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    e.as_string().unwrap_or_else(|| "embed failed".into()),
                    None,
                )
            })?;
        Ok(batch.into_iter().next().unwrap_or_default())
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        _model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        self.embed_texts(texts.to_vec()).await.map_err(|e| {
            QqlError::execution(
                "QQL-EMBEDDING",
                e.as_string().unwrap_or_else(|| "embed batch failed".into()),
                None,
            )
        })
    }

    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError> {
        Ok(qql_embed::sparse::build_query_default(text))
    }
}
