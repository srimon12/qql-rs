//! Free functions: parsing, analysis, compilation, and binding.

use qql_core::ast::{self, ComparisonOp};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_plan::routing;
use wasm_bindgen::prelude::*;

thread_local! {
    pub(crate) static SCRATCH_BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::with_capacity(8192));
}

/// Safely copies slice into a JS-owned Uint8Array without borrowing WASM
/// memory. `Uint8Array::from` copies via the JS API, so there is no
/// `Uint8Array::view` lifetime to invalidate on memory growth.
pub(crate) fn safe_owned_uint8_array(bytes: &[u8]) -> js_sys::Uint8Array {
    js_sys::Uint8Array::from(bytes)
}

#[wasm_bindgen(unchecked_return_type = "unknown[]")]
pub fn parse(input: &str) -> Result<JsValue, JsValue> {
    // Always parse as a script — returns a list even for single statements.
    let stmts = Parser::parse_all(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&stmts)
}

/// Parse to a raw JSON string of the AST array — no JS object allocation,
/// mirroring `parseJson` on the Node SDK.
#[wasm_bindgen(js_name = parseJson, unchecked_return_type = "string")]
pub fn parse_json(input: &str) -> Result<String, JsValue> {
    let stmts = Parser::parse_all(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&stmts).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = isValid)]
pub fn is_valid(input: &str) -> bool {
    // Full frontend gate: parse + plan — same contract as execution and the
    // language conformance suite.
    qql_plan::parse_and_plan(input).is_ok()
}

#[wasm_bindgen]
pub fn inject_filter(
    query: &str,
    field: &str,
    op: &str,
    value: JsValue,
) -> Result<JsValue, JsValue> {
    let val =
        super::params::jsvalue_to_value(&value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let cmp = parse_comparison_op(op)?;
    let mut stmt = Parser::parse(query).map_err(|e| JsValue::from_str(&e.to_string()))?;
    ast::inject_filter(&mut stmt, field, cmp, val)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&stmt)
}

pub(crate) fn parse_comparison_op(op: &str) -> Result<ComparisonOp, JsValue> {
    // Single source in qql-core: supported operators and rejection messages
    // stay identical across every SDK binding.
    qql_core::ast::ComparisonOp::parse_inject_op(op).map_err(|e| JsValue::from_str(&e.to_string()))
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

fn err_json(err: &qql_core::error::QqlError) -> serde_json::Value {
    serde_json::json!({
        "code": err.code.as_ref(),
        "message": err.message.as_ref(),
        "start": err.span.map(|s| s.start),
        "end": err.span.map(|s| s.end),
    })
}

fn build_analyze_value(input: &str) -> serde_json::Value {
    let mut tokens = Vec::new();
    let lexer = Lexer::new(input);
    for token in lexer {
        // The first lex error ends the token stream; recovery parse below
        // reports it (never iterate with `flatten()` — it would silently
        // drop the error instead of stopping).
        let Ok(t) = token else { break };
        tokens.push(serde_json::json!({
            "kind": t.kind.as_str(),
            "text": t.text,
            "pos": t.span.start,
            "end": t.span.end,
            "len": t.span.end.saturating_sub(t.span.start),
        }));
    }

    // `error` / `valid` stay on the fail-fast execution contract
    // (`parse_and_plan`) so language-corpus `-- @error` codes and older
    // clients keep seeing the first rejection. `errors` is the panic-mode
    // recovery list for IDEs that want every span.
    let fail_fast = qql_plan::parse_and_plan(input);
    let recovered = Parser::parse_all_recovering(input);
    let mut errors: Vec<serde_json::Value> = recovered.errors.iter().map(err_json).collect();
    let stmts: Vec<_> = recovered
        .statements
        .into_iter()
        .map(|(stmt, _)| stmt)
        .collect();
    let mut routes_val = Vec::new();
    for stmt in &stmts {
        match routing::compile_statement(stmt) {
            Ok(compiled) => routes_val.push(compiled_route_json(&compiled)),
            Err(err) => errors.push(err_json(&err)),
        }
    }
    let fail_fast_error = fail_fast.as_ref().err().map(err_json);
    if errors.is_empty()
        && let Some(err) = fail_fast_error.clone()
    {
        errors.push(err);
    }
    let ast_val = if stmts.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::to_value(&stmts).unwrap_or(serde_json::Value::Null)
    };
    let explain_val = if stmts.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(qql_core::explain::explain_nodes(&stmts))
    };
    let route_val = routes_val
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "valid": fail_fast.is_ok(),
        "statements_count": stmts.len(),
        "tokens": tokens,
        "ast": ast_val,
        "route": route_val,
        "routes": routes_val,
        "explain": explain_val,
        "error": fail_fast_error.unwrap_or(serde_json::Value::Null),
        "errors": errors,
    })
}

pub(crate) fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
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

pub(crate) fn compiled_route_json(compiled: &qql_plan::CompiledStatement) -> serde_json::Value {
    match &compiled.route {
        Some(route) => serde_json::json!({
            "stmt_type": compiled.stmt_type,
            "method": route.method.as_str(),
            "path": route.path,
            "payload": route.body_json().unwrap_or(serde_json::Value::Null),
        }),
        None => serde_json::json!({
            "stmt_type": compiled.stmt_type,
            "method": serde_json::Value::Null,
            "path": serde_json::Value::Null,
            "payload": serde_json::Value::Null,
        }),
    }
}

fn build_compile_output(
    query: &str,
    params: Option<JsValue>,
) -> Result<serde_json::Value, JsValue> {
    let bound = match params {
        Some(p) if !p.is_undefined() && !p.is_null() => {
            let parsed = super::params::jsvalue_to_value(&p)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            super::params::bind_value_params(query, &parsed, false)?
        }
        _ => query.to_string(),
    };
    let stmt = Parser::parse(&bound).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let compiled =
        routing::compile_statement(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(compiled_route_json(&compiled))
}

/// Compile one QQL statement into a JavaScript route object. Optional
/// `params` (object for `:name`, array for `?`) bind before parsing —
/// parity with `Client.compile(query, params)` on the Python and Node SDKs.
#[wasm_bindgen(unchecked_return_type = "CompiledRoute")]
pub fn compile(query: &str, params: Option<JsValue>) -> Result<JsValue, JsValue> {
    let output = build_compile_output(query, params)?;
    to_js_value(&output)
}

/// Compile one QQL statement into a JavaScript route object. Alias for `compile`.
#[wasm_bindgen(js_name = compileQuery, unchecked_return_type = "CompiledRoute")]
pub fn compile_query(query: &str, params: Option<JsValue>) -> Result<JsValue, JsValue> {
    compile(query, params)
}

/// Compiles QQL query into a safe, JS-owned Uint8Array byte buffer.
#[wasm_bindgen(js_name = compileBytes)]
pub fn compile_bytes(query: &str) -> Result<js_sys::Uint8Array, JsValue> {
    let output = build_compile_output(query, None)?;
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

/// Format a QQL string into canonical form.
#[wasm_bindgen(js_name = formatQuery)]
pub fn format_query(input: &str) -> Result<String, JsValue> {
    qql_core::fmt::format(input).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Substitute `:name` (object) or `?` (array) placeholders into a query string.
/// Without `params`, the query is returned unchanged (mirrors pyqql `bind`).
#[wasm_bindgen]
pub fn bind(
    query: &str,
    #[wasm_bindgen(unchecked_optional_param_type = "Record<string, unknown> | unknown[]")]
    params: Option<JsValue>,
    #[wasm_bindgen(unchecked_optional_param_type = "{ truncateVectors?: boolean }")]
    options: Option<JsValue>,
) -> Result<String, JsValue> {
    let truncate = options
        .as_ref()
        .and_then(|o| js_sys::Reflect::get(o, &JsValue::from_str("truncateVectors")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match params {
        Some(p) if !p.is_null() && !p.is_undefined() => {
            let parsed = super::params::jsvalue_to_value(&p)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            super::params::bind_value_params(query, &parsed, truncate)
        }
        _ => Ok(query.to_string()),
    }
}
