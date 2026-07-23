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

// ── Core: parsing ────────────────────────────────────────────────

fn normalize_input(input: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = input.trim();
    if !trimmed.is_empty() && !trimmed.ends_with(';') {
        std::borrow::Cow::Owned(format!("{};", trimmed))
    } else {
        std::borrow::Cow::Borrowed(trimmed)
    }
}

#[wasm_bindgen]
pub fn parse(input: &str) -> Result<JsValue, JsValue> {
    let norm = normalize_input(input);
    let stmt = Parser::parse(&norm).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn parse_all(input: &str) -> Result<JsValue, JsValue> {
    let norm = normalize_input(input);
    let stmts = Parser::parse_all(&norm).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&stmts).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn parse_batch(queries: Vec<String>) -> Result<JsValue, JsValue> {
    let results = js_sys::Array::new();
    for q in queries {
        let norm = normalize_input(&q);
        let stmt = Parser::parse(&norm).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let v =
            serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))?;
        results.push(&v);
    }
    Ok(results.into())
}

#[wasm_bindgen(js_name = isValid)]
pub fn is_valid(input: &str) -> bool {
    let norm = normalize_input(input);
    Parser::try_parse(&norm).is_ok()
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
    serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))
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

    /// Get or set the shard key (QUERY, COUNT, SCROLL, UPSERT, DELETE only).
    #[wasm_bindgen(getter, js_name = shardKey)]
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

    #[wasm_bindgen(setter, js_name = shardKey)]
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

    /// Serialise the AST to a JSON string.
    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.inner).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Serialise the AST to a JS object.
    #[wasm_bindgen(js_name = toObject)]
    pub fn to_object(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Compile this Stmt AST directly into a Qdrant REST route JSON payload.
    #[wasm_bindgen(js_name = compileRoute)]
    pub fn compile_route(&self) -> Result<String, JsValue> {
        let route = routing::route(&self.inner);
        let json_body = route.body_json();
        let output = serde_json::json!({
            "method": route.method.as_str(),
            "path": route.path,
            "payload": json_body.unwrap_or(serde_json::Value::Null),
        });
        serde_json::to_string_pretty(&output).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

// ── Core: tokenize ────────────────────────────────────────────────

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

#[wasm_bindgen]
pub fn analyze(input: &str) -> String {
    let norm = normalize_input(input);
    let mut tokens = Vec::new();
    let lexer = Lexer::new(&norm);
    for t in lexer.flatten() {
        tokens.push(serde_json::json!({
            "kind": t.kind.as_str(),
            "text": t.text,
            "pos": t.span.start,
            "end": t.span.end,
            "len": t.span.end.saturating_sub(t.span.start),
        }));
    }

    let stmts_res = Parser::parse_all(&norm);
    match stmts_res {
        Ok(stmts) => {
            let ast_val = serde_json::to_value(&stmts).unwrap_or(serde_json::Value::Null);
            let routes_val: Vec<_> = stmts
                .iter()
                .map(|s| {
                    let r = routing::route(s);
                    serde_json::json!({
                        "method": r.method.as_str(),
                        "path": r.path,
                        "payload": r.body_json().unwrap_or(serde_json::Value::Null),
                    })
                })
                .collect();
            let route_val = routes_val.first().cloned().unwrap_or(serde_json::Value::Null);

            let explain_val = qql_core::explain::explain(&norm).unwrap_or_default();

            let result = serde_json::json!({
                "valid": true,
                "statements_count": stmts.len(),
                "tokens": tokens,
                "ast": ast_val,
                "route": route_val,
                "routes": routes_val,
                "explain": explain_val,
                "error": serde_json::Value::Null,
            });

            serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
        }
        Err(err) => {
            let err_json = serde_json::json!({
                "code": err.code.as_ref(),
                "message": err.message.as_ref(),
                "start": err.span.map(|s| s.start),
                "end": err.span.map(|s| s.end),
            });

            let result = serde_json::json!({
                "valid": false,
                "statements_count": 0,
                "tokens": tokens,
                "ast": serde_json::Value::Null,
                "route": serde_json::Value::Null,
                "explain": serde_json::Value::Null,
                "error": err_json,
            });

            serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
        }
    }
}

// ── Core: compile & explain ───────────────────────────────────────

#[wasm_bindgen]
pub fn compile(query: &str) -> Result<String, JsValue> {
    let norm = normalize_input(query);
    let stmt = Parser::parse(&norm).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let route = routing::route(&stmt);
    let json_body = route.body_json();
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
        "payload": json_body.unwrap_or(serde_json::Value::Null),
    });
    serde_json::to_string(&output).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn explain(query: &str) -> Result<String, JsValue> {
    let norm = normalize_input(query);
    qql_core::explain::explain(&norm).map_err(|e| JsValue::from_str(&e.to_string()))
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
    pub fn set_embedder(&mut self, fn_: js_sys::Function) {
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
                let promise = fn_
                    .call1(&JsValue::NULL, &array)
                    .map_err(|e| JsValue::from_str(&format!("embedder call failed: {:?}", e)))?;

                let result = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(promise))
                    .await
                    .map_err(|e| JsValue::from_str(&format!("embedder rejected: {:?}", e)))?;

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
    /// Accepts:
    /// - `string` — single statement or semicolon-delimited multi-statement
    ///   script (smart batching for same-collection queries/mutations)
    /// - `string[]` — each entry executed as above; results returned as array
    #[wasm_bindgen]
    pub async fn execute(&self, query: JsValue) -> Result<String, JsValue> {
        if js_sys::Array::is_array(&query) {
            let arr = js_sys::Array::from(&query);
            let len = arr.length() as usize;
            let mut results: Vec<serde_json::Value> = Vec::with_capacity(len);
            for i in 0..len {
                let item = arr.get(i as u32);
                if let Some(s) = item.as_string() {
                    match self.execute_script(&s).await {
                        Ok(v) => results.push(v),
                        Err(e) => {
                            results.push(serde_json::json!({
                                "ok": false,
                                "error": e.as_string().unwrap_or_default()
                            }));
                        }
                    }
                }
            }
            return serde_json::to_string(&results).map_err(|e| JsValue::from_str(&e.to_string()));
        }

        if let Some(s) = query.as_string() {
            let val = self.execute_script(&s).await?;
            return serde_json::to_string(&val).map_err(|e| JsValue::from_str(&e.to_string()));
        }

        Err(JsValue::from_str("query must be a string or string[]"))
    }

    /// Execute a pre-parsed Stmt object.  Injects embeddings for UPSERT
    /// if an embedder is configured.
    #[wasm_bindgen(js_name = executeStmt)]
    pub async fn execute_stmt(&self, stmt: &Stmt) -> Result<String, JsValue> {
        let val = self.execute_stmt_inner(&stmt.inner).await?;
        serde_json::to_string(&val).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Execute one or more statements with order-preserving smart batching.
    async fn execute_script(&self, query: &str) -> Result<serde_json::Value, JsValue> {
        let norm = normalize_input(query);
        let stmts = match Parser::parse_all(&norm) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                return Ok(serde_json::json!({
                    "ok": true,
                    "operation": "EMPTY",
                    "message": "empty script",
                }));
            }
            Err(_) => {
                // Fall back to single-statement parse for better error messages
                let stmt = Parser::parse(&norm).map_err(|e| JsValue::from_str(&e.to_string()))?;
                vec![stmt]
            }
        };

        if stmts.len() == 1 {
            return self.execute_stmt_inner(&stmts[0]).await;
        }

        let mut results = Vec::with_capacity(stmts.len());
        let mut i = 0;
        while i < stmts.len() {
            // Contiguous mutation batch
            if let Some((coll, first_op)) = qql_plan::mutation::lower_update_operation(&stmts[i]) {
                let mut ops = vec![first_op];
                let mut j = i + 1;
                while j < stmts.len() {
                    match qql_plan::mutation::lower_update_operation(&stmts[j]) {
                        Some((c, op)) if c == coll => {
                            ops.push(op);
                            j += 1;
                        }
                        _ => break,
                    }
                }
                if ops.len() >= 2 {
                    let op_names: Vec<&'static str> =
                        ops.iter().map(|o| o.operation_name()).collect();
                    let batch = qql_plan::UpdateBatchRequest { operations: ops };
                    let path = format!("/collections/{coll}/points/batch?wait=true");
                    let body = serde_json::to_value(&batch)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    let resp = self.send_json("POST", &path, Some(body)).await?;
                    let arr = resp
                        .get("result")
                        .and_then(|r| r.as_array())
                        .cloned()
                        .unwrap_or_default();
                    for (k, val) in arr.into_iter().enumerate() {
                        results.push(serde_json::json!({
                            "ok": true,
                            "operation": op_names.get(k).copied().unwrap_or("MUTATION"),
                            "data": val,
                        }));
                    }
                    while results.len() < j {
                        let name = op_names
                            .get(results.len().saturating_sub(i))
                            .copied()
                            .unwrap_or("MUTATION");
                        results.push(serde_json::json!({"ok": true, "operation": name}));
                    }
                    i = j;
                    continue;
                }
            }

            // Contiguous query batch
            if let Some((coll, q0)) = wasm_batchable_query(&stmts[i]) {
                let req0 = qql_plan::query::lower_query_request(&q0)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let mut searches = vec![req0];
                let mut j = i + 1;
                while j < stmts.len() {
                    match wasm_batchable_query(&stmts[j]) {
                        Some((c, q)) if c == coll => {
                            let req = qql_plan::query::lower_query_request(&q)
                                .map_err(|e| JsValue::from_str(&e.to_string()))?;
                            searches.push(req);
                            j += 1;
                        }
                        _ => break,
                    }
                }
                if searches.len() >= 2 {
                    let n = searches.len();
                    let batch = qql_plan::QueryBatchRequest { searches };
                    let path = format!("/collections/{coll}/points/query/batch");
                    let body = serde_json::to_value(&batch)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    let resp = self.send_json("POST", &path, Some(body)).await?;
                    let arr = resp
                        .get("result")
                        .and_then(|r| r.as_array())
                        .cloned()
                        .unwrap_or_default();
                    for val in arr {
                        results.push(serde_json::json!({
                            "ok": true,
                            "operation": "QUERY",
                            "data": val,
                        }));
                    }
                    while results.len() < i + n {
                        results.push(serde_json::json!({
                            "ok": true,
                            "operation": "QUERY",
                            "data": {"points": []},
                        }));
                    }
                    i = j;
                    continue;
                }
            }

            match self.execute_stmt_inner(&stmts[i]).await {
                Ok(v) => results.push(v),
                Err(e) => {
                    results.push(serde_json::json!({
                        "ok": false,
                        "error": e.as_string().unwrap_or_default(),
                    }));
                }
            }
            i += 1;
        }

        Ok(serde_json::json!({
            "ok": true,
            "operation": "SCRIPT",
            "message": format!("Executed {} statement(s)", results.len()),
            "data": results,
        }))
    }

    async fn execute_stmt_inner(
        &self,
        stmt: &qql_core::ast::Stmt,
    ) -> Result<serde_json::Value, JsValue> {
        let mut stmt = stmt.clone();
        self.resolve_stmt_embeddings(&mut stmt).await?;
        let route = routing::route(&stmt);
        let body = route.body_json();
        self.send_json(route.method.as_str(), &route.path, body)
            .await
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

    /// Parse → route → return the JSON payload without executing.
    #[wasm_bindgen]
    pub fn compile(&self, query: &str) -> Result<String, JsValue> {
        let stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let route = routing::route(&stmt);
        let json_body = route.body_json();
        let output = serde_json::json!({
            "method": route.method.as_str(),
            "path": route.path,
            "payload": json_body.unwrap_or(serde_json::Value::Null),
        });
        serde_json::to_string(&output).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Parse and explain the query — no server needed.
    #[wasm_bindgen]
    pub fn explain(&self, query: &str) -> Result<String, JsValue> {
        qql_core::explain::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
fn wasm_batchable_query(stmt: &qql_core::ast::Stmt) -> Option<(String, qql_core::ast::QueryStmt)> {
    match stmt {
        qql_core::ast::Stmt::Query(q) => {
            if matches!(q.expression, qql_core::ast::QueryExpr::Points { .. }) || q.group.is_some()
            {
                return None;
            }
            match &q.collection {
                qql_core::ast::QueryCollection::Explicit(name) => {
                    Some((name.clone(), (**q).clone()))
                }
                qql_core::ast::QueryCollection::Inherited => None,
            }
        }
        _ => None,
    }
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
