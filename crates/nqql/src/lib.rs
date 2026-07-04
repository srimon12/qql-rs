use std::borrow::Cow;

use napi_derive::napi;
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[napi]
pub fn parse(input: String) -> napi::Result<String> {
    match Parser::parse(&input) {
        Ok(stmt) => Ok(format!("{:#?}", stmt)),
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
}

#[napi]
pub fn parse_all(input: String) -> napi::Result<Vec<String>> {
    match Parser::parse_all(&input) {
        Ok(stmts) => Ok(stmts.into_iter().map(|s| format!("{:#?}", s)).collect()),
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
}

#[napi]
pub fn parse_batch(queries: Vec<String>) -> napi::Result<Vec<String>> {
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        match Parser::parse(&q) {
            Ok(stmt) => results.push(format!("{:#?}", stmt)),
            Err(e) => return Err(napi::Error::from_reason(e.to_string())),
        }
    }
    Ok(results)
}

#[napi]
pub fn is_valid(input: String) -> bool {
    Parser::parse(&input).is_ok()
}

#[napi]
pub fn inject_filter(
    query: String,
    field: String,
    op: String,
    value_json: String,
) -> napi::Result<String> {
    let value =
        json_to_value(&value_json).ok_or_else(|| napi::Error::from_reason("invalid value JSON"))?;
    let mut stmt = Parser::parse(&query).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    ast::inject_filter(&mut stmt, &field, &op, &value);
    Ok(format!("{:#?}", stmt))
}

#[napi]
pub fn tokenize(input: String) -> napi::Result<String> {
    let lexer = Lexer::new(&input);
    let mut tokens = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(|e| napi::Error::from_reason(e.to_string()))?;
        tokens.push(serde_json::json!({
            "kind": token.kind.as_str(),
            "text": token.text,
            "pos": token.pos,
        }));
    }
    serde_json::to_string(&tokens)
        .map_err(|e| napi::Error::from_reason(format!("failed to serialize tokens: {}", e)))
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
            } else {
                n.as_f64().map(Value::Float)
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
