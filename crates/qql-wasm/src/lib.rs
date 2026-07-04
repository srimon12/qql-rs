use std::borrow::Cow;

use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn parse(input: &str) -> Result<String, JsValue> {
    let stmt = Parser::parse(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(format!("{:#?}", stmt))
}

#[wasm_bindgen]
pub fn is_valid(input: &str) -> bool {
    Parser::parse(input).is_ok()
}

#[wasm_bindgen]
pub fn inject_filter(query: &str, field: &str, op: &str, value_json: &str) -> Result<String, JsValue> {
    let value = json_to_value(value_json)
        .ok_or_else(|| JsValue::from_str("invalid value JSON"))?;
    let mut stmt = Parser::parse(query)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    ast::inject_filter(&mut stmt, field, op, &value);
    Ok(format!("{:#?}", stmt))
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

fn json_to_value(json: &str) -> Option<Value<'static>> {
    let jv: serde_json::Value = serde_json::from_str(json).ok()?;
    serde_json_to_value(jv)
}

fn serde_json_to_value(jv: serde_json::Value) -> Option<Value<'static>> {
    match jv {
        serde_json::Value::String(s) => Some(Value::Str(Cow::Owned(s))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Some(Value::Float(f))
            } else {
                None
            }
        }
        serde_json::Value::Bool(b) => Some(Value::Bool(b)),
        serde_json::Value::Null => Some(Value::Null),
        serde_json::Value::Array(items) => {
            let mut vals = Vec::with_capacity(items.len());
            for item in items {
                vals.push(serde_json_to_value(item)?);
            }
            Some(Value::List(vals))
        }
        serde_json::Value::Object(map) => {
            let mut pairs = Vec::with_capacity(map.len());
            for (k, v) in map {
                pairs.push((Cow::Owned(k), serde_json_to_value(v)?));
            }
            Some(Value::Dict(pairs))
        }
    }
}
