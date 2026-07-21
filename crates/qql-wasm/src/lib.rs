use gloo_net::http::Request;
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_plan::routing;
use serde_json::json;
use wasm_bindgen::prelude::*;

// ── Core: parsing ────────────────────────────────────────────────

#[wasm_bindgen]
pub fn parse(input: &str) -> Result<JsValue, JsValue> {
    let stmt = Parser::parse(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn parse_all(input: &str) -> Result<JsValue, JsValue> {
    let stmts = Parser::parse_all(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&stmts).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn parse_batch(queries: Vec<String>) -> Result<JsValue, JsValue> {
    let results = js_sys::Array::new();
    for q in queries {
        let stmt = Parser::parse(&q).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let v =
            serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))?;
        results.push(&v);
    }
    Ok(results.into())
}

#[wasm_bindgen(js_name = isValid)]
pub fn is_valid(input: &str) -> bool {
    Parser::try_parse(input).is_ok()
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
    let cmp = match op {
        "=" | "==" | "eq" => ComparisonOp::Eq,
        "!=" | "neq" => ComparisonOp::Eq,
        ">" | "gt" => ComparisonOp::Gt,
        ">=" | "gte" => ComparisonOp::Gte,
        "<" | "lt" => ComparisonOp::Lt,
        "<=" | "lte" => ComparisonOp::Lte,
        _ => ComparisonOp::Eq,
    };
    let mut stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    ast::inject_filter(&mut stmt, field, cmp, val)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))
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
        tokens.push(JsValue::from(obj));
    }
    Ok(tokens)
}

// ── Core: compile & explain ───────────────────────────────────────

#[wasm_bindgen]
pub fn compile(query: &str) -> Result<String, JsValue> {
    let stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
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
            Some(qql_plan::routing::RequestBody::CreateCollection(_)) => "create_collection",
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
    qql_core::explain::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
}

// ── Client: browser fetch-based execute with embedding ────────────

enum EmbedMode {
    None,
    /// JS function: `async (texts: string[]) => number[][]`
    Js(js_sys::Function),
    /// OpenAI-compatible: POST with Bearer auth, {"input": texts, "model": name}
    /// response: {"data": [{"embedding": [...]}, ...]}
    OpenAI,
    /// Generic HTTP: POST with optional Bearer auth, custom body/response paths
    Http,
}

#[wasm_bindgen]
pub struct Client {
    url: String,
    api_key: Option<String>,

    // Embedding config
    embed_mode: EmbedMode,
    embed_endpoint: String,
    embed_api_key: Option<String>,
    embed_model: String,
    embed_dim: u32,
    // Custom embedder: request body field name for texts (default: "input")
    #[allow(dead_code)]
    embed_request_field: String,
    // Custom embedder: JSON path to vectors array (default: "data[*].embedding")
    #[allow(dead_code)]
    embed_response_path: String,
}

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
            embed_request_field: "input".to_string(),
            embed_response_path: "data[*].embedding".to_string(),
        }
    }

    // ── Embedder configuration ──────────────────────────────────

    /// Set a JS embedder function: `async (texts: string[]) => number[][]`.
    /// Use for Transformer.js or any browser-side embedding.
    #[wasm_bindgen(js_name = setEmbedder)]
    pub fn set_embedder(&mut self, fn_: js_sys::Function) {
        self.embed_mode = EmbedMode::Js(fn_);
    }

    /// Set OpenAI-compatible embedding (e.g. OpenAI, Mistral, Cohere, local Ollama).
    /// Uses Bearer auth. POSTs `{"model": model, "input": texts}`.
    #[wasm_bindgen(js_name = setOpenAIEmbedder)]
    pub fn set_openai_embedder(
        &mut self,
        api_key: String,
        model: String,
        dimensions: Option<u32>,
        endpoint: Option<String>,
    ) {
        self.embed_mode = EmbedMode::OpenAI;
        self.embed_endpoint =
            endpoint.unwrap_or_else(|| "https://api.openai.com/v1/embeddings".to_string());
        self.embed_api_key = Some(api_key);
        self.embed_model = model;
        self.embed_dim = dimensions.unwrap_or(0);
    }

    /// Set a generic remote HTTP embedding endpoint.
    /// POSTs `{"model": model, "<input field>": texts}` with optional Bearer auth.
    /// Expects vectors in response at the configured JSON path.
    #[wasm_bindgen(js_name = setRemoteEmbedder)]
    pub fn set_remote_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) {
        self.embed_mode = EmbedMode::Http;
        self.embed_endpoint = endpoint;
        self.embed_api_key = api_key;
        self.embed_model = model;
        self.embed_dim = dimension;
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
    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f64>>, JsValue> {
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

                let rows: Vec<Vec<f64>> = serde_wasm_bindgen::from_value(result).map_err(|e| {
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

            EmbedMode::OpenAI => {
                let body = json!({ "model": self.embed_model, "input": texts });
                let resp = self.post_with_auth(&self.embed_endpoint, &body).await?;
                Self::parse_openai_response(&resp, texts.len())
            }

            EmbedMode::Http => {
                let body = json!({ "model": self.embed_model, "input": texts });
                let resp = self.post_with_auth(&self.embed_endpoint, &body).await?;
                Self::parse_remote_response(&resp, texts.len(), &self.embed_response_path)
                    .map_err(|e| JsValue::from_str(&e))
            }

            EmbedMode::None => Ok(Vec::new()),
        }
    }

    /// POST JSON with Bearer auth to embedding endpoint.
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

    /// Parse OpenAI-style: `{"data": [{"embedding": [...], "index": 0}, ...]}`
    fn parse_openai_response(
        resp: &serde_json::Value,
        expected: usize,
    ) -> Result<Vec<Vec<f64>>, JsValue> {
        let data = resp["data"]
            .as_array()
            .ok_or_else(|| JsValue::from_str("OpenAI response missing 'data' array"))?;
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let emb = item["embedding"]
                .as_array()
                .ok_or_else(|| JsValue::from_str("item missing 'embedding' array"))?;
            out.push(emb.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect());
        }
        if out.len() != expected {
            return Err(JsValue::from_str(&format!(
                "got {} vectors, expected {}",
                out.len(),
                expected
            )));
        }
        Ok(out)
    }

    /// Parse a generic HTTP response at a JSON path like `data[*].embedding`.
    fn parse_remote_response(
        resp: &serde_json::Value,
        expected: usize,
        path: &str,
    ) -> Result<Vec<Vec<f64>>, String> {
        let parts: Vec<&str> = path.split('.').collect();
        let star_pos = parts.iter().position(|p| *p == "[*]");

        if let Some(pos) = star_pos {
            // Walk to the array
            let mut cur = resp;
            for p in &parts[..pos] {
                cur = cur
                    .get(*p)
                    .ok_or_else(|| format!("missing '{}' in response", p))?;
            }
            let arr = cur
                .as_array()
                .ok_or_else(|| "expected array at [*]".to_string())?;

            // Remaining path on each element
            let tail = parts[pos + 1..].join(".");
            let mut out = Vec::new();
            for item in arr {
                let v = Self::walk_path(item, &tail)?;
                out.push(v);
            }
            if out.len() != expected {
                return Err(format!("got {} vectors, expected {}", out.len(), expected));
            }
            Ok(out)
        } else {
            let v = Self::walk_path(resp, path)?;
            Ok(vec![v])
        }
    }

    fn walk_path(val: &serde_json::Value, path: &str) -> Result<Vec<f64>, String> {
        let mut cur = val;
        for p in path.split('.') {
            cur = cur.get(p).ok_or_else(|| format!("missing '{}'", p))?;
        }
        cur.as_array()
            .ok_or_else(|| "expected array".to_string())?
            .iter()
            .map(|v| v.as_f64().ok_or_else(|| "non-numeric".to_string()))
            .collect()
    }

    /// Extract texts that need embedding from upsert JSON points,
    /// embed them, and inject the resulting vectors back into the payload.
    async fn embed_upsert_points(&self, payload: &mut serde_json::Value) -> Result<(), JsValue> {
        if !self.has_embedder() {
            return Ok(());
        }

        let points = match payload["points"].as_array_mut() {
            Some(p) => p,
            None => return Ok(()),
        };

        // Collect all texts that need embedding across points
        let mut text_indices: Vec<(usize, String)> = Vec::new();
        for (pi, point) in points.iter().enumerate() {
            // Skip points that already have vectors
            if point.get("vector").is_some() {
                continue;
            }
            if let Some(text) = point["payload"]["text"].as_str() {
                if !text.is_empty() {
                    text_indices.push((pi, text.to_string()));
                }
            }
        }

        if text_indices.is_empty() {
            return Ok(());
        }

        let texts: Vec<String> = text_indices.iter().map(|(_, t)| t.clone()).collect();
        let vectors = self.embed_texts(texts).await?;

        // Inject vectors back into the points
        for (idx, (pi, _)) in text_indices.iter().enumerate() {
            let vec = &vectors[idx];
            let vec_json: Vec<serde_json::Value> = vec.iter().map(|n| json!(n)).collect();
            points[*pi]["vector"] = json!(vec_json);
        }

        Ok(())
    }

    /// Parse, compile, embed if needed, and POST to Qdrant's REST API.
    #[wasm_bindgen]
    pub async fn execute(&self, query: &str) -> Result<JsValue, JsValue> {
        let stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let route = routing::route(&stmt);
        let method_str = route.method.as_str();
        let path = &route.path;

        let mut body = route.body_json();

        // Inject embeddings for upsert statements if embedder is configured
        if matches!(stmt, qql_core::ast::Stmt::Upsert(_)) {
            if let Some(ref mut payload) = body {
                self.embed_upsert_points(payload).await?;
            }
        }

        let body_str = body
            .as_ref()
            .map(|b| serde_json::to_string(b).map_err(|e| JsValue::from_str(&e.to_string())))
            .transpose()?;

        let rb = self.request(method_str, path);
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

        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|e| JsValue::from_str(&e.to_string()))
            .and_then(|v| {
                serde_wasm_bindgen::to_value(&v).map_err(|e| JsValue::from_str(&e.to_string()))
            })
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
