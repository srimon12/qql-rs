use std::borrow::Cow;

use napi_derive::napi;
use qql::offline;
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[napi]
pub fn parse(input: String) -> napi::Result<serde_json::Value> {
    match Parser::parse(&input) {
        Ok(stmt) => {
            serde_json::to_value(&stmt).map_err(|e| napi::Error::from_reason(e.to_string()))
        }
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
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
pub fn parse_all(input: String) -> napi::Result<serde_json::Value> {
    match Parser::parse_all(&input) {
        Ok(stmts) => {
            serde_json::to_value(&stmts).map_err(|e| napi::Error::from_reason(e.to_string()))
        }
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
}

#[napi]
pub fn parse_batch(queries: Vec<String>) -> napi::Result<serde_json::Value> {
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        match Parser::parse(&q) {
            Ok(stmt) => results.push(
                serde_json::to_value(&stmt).map_err(|e| napi::Error::from_reason(e.to_string()))?,
            ),
            Err(e) => return Err(napi::Error::from_reason(e.to_string())),
        }
    }
    serde_json::to_value(&results).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
pub fn parse_batch_json(queries: Vec<String>) -> napi::Result<String> {
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        match Parser::parse(&q) {
            Ok(stmt) => results.push(
                serde_json::to_value(&stmt).map_err(|e| napi::Error::from_reason(e.to_string()))?,
            ),
            Err(e) => return Err(napi::Error::from_reason(e.to_string())),
        }
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
        serde_json_to_value(value).ok_or_else(|| napi::Error::from_reason("invalid value JSON"))?;
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
            if map.len() == 1 {
                if let Some((tag, inner)) = map.iter().next() {
                    match tag.as_str() {
                        "str" => return inner.as_str().map(|s| Value::Str(Cow::Owned(s.into()))),
                        "int" => return inner.as_i64().map(Value::Int),
                        "float" => return inner.as_f64().map(Value::Float),
                        "bool" => return inner.as_bool().map(Value::Bool),
                        "null" if inner.is_null() => return Some(Value::Null),
                        "list" => return serde_json_to_value(inner.clone()),
                        "dict" => return serde_json_to_value(inner.clone()),
                        _ => {}
                    }
                }
            }
            let mut pairs = Vec::with_capacity(map.len());
            for (k, v) in map {
                pairs.push((Cow::Owned(k), serde_json_to_value(v)?));
            }
            Some(Value::Dict(pairs))
        }
    }
}
