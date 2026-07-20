use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

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
    let value =
        Value::from_json(serde_value).ok_or_else(|| JsValue::from_str("unsupported value type"))?;
    let mut stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    ast::inject_filter(&mut stmt, field, op, &value);
    serde_wasm_bindgen::to_value(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))
}

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
            &JsValue::from_f64(token.pos as f64),
        )
        .unwrap();
        tokens.push(JsValue::from(obj));
    }
    Ok(tokens)
}

#[wasm_bindgen]
pub fn compile(query: &str) -> Result<JsValue, JsValue> {
    let compiled =
        qql_core::offline::compile(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&compiled).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(feature = "runtime")]
#[wasm_bindgen]
pub fn explain(query: &str) -> Result<String, JsValue> {
    q::executor::Executor::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(feature = "runtime")]
#[wasm_bindgen]
pub struct HttpEmbedder {
    endpoint: String,
    api_key: String,
    model: String,
    dimension: usize,
}

#[cfg(feature = "runtime")]
#[wasm_bindgen]
impl HttpEmbedder {
    #[wasm_bindgen(constructor)]
    pub fn new(
        endpoint: &str,
        model: &str,
        dimension: usize,
        api_key: Option<String>,
    ) -> HttpEmbedder {
        HttpEmbedder {
            endpoint: endpoint.to_string(),
            api_key: api_key.unwrap_or_default(),
            model: model.to_string(),
            dimension,
        }
    }

    #[wasm_bindgen(getter)]
    pub fn endpoint(&self) -> String {
        self.endpoint.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn model(&self) -> String {
        self.model.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(feature = "runtime")]
#[wasm_bindgen]
pub struct Client {
    url: String,
    api_key: Option<String>,
    embedder: Option<HttpEmbedder>,
}

#[cfg(feature = "runtime")]
#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        embedder: Option<HttpEmbedder>,
    ) -> Client {
        Client {
            url: url.unwrap_or_else(|| "http://localhost:6333".to_string()),
            api_key,
            embedder,
        }
    }

    #[wasm_bindgen]
    pub fn compile(&self, query: &str) -> Result<JsValue, JsValue> {
        let compiled =
            qql_core::offline::compile(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&compiled).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen]
    pub fn explain(&self, query: &str) -> Result<String, JsValue> {
        q::executor::Executor::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
