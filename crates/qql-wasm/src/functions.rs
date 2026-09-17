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

/// Map a [`QqlError`](qql_core::error::QqlError) to a JS error value carrying
/// structured `.code` / `.kind` / `.span` / `.fields`.
///
/// `dx.js buildError` recovers those fields via `JSON.parse` — a plain
/// `Display` string (`"[CODE] message"`) does NOT survive, so every
/// `QqlError` thrown to JS must go through here. Non-QQL failures (serde,
/// host traps, transport strings) keep their `Display` mapping at each
/// call site.
pub(crate) fn qql_err_to_js(err: qql_core::error::QqlError) -> JsValue {
    JsValue::from_str(&serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()))
}

#[wasm_bindgen(unchecked_return_type = "unknown[]")]
pub fn parse(input: &str) -> Result<JsValue, JsValue> {
    // Always parse as a script — returns a list even for single statements.
    let stmts = Parser::parse_all(input).map_err(qql_err_to_js)?;
    to_js_value(&stmts)
}

/// Parse to a raw JSON string of the AST array — no JS object allocation,
/// mirroring `parseJson` on the Node SDK.
#[wasm_bindgen(js_name = parseJson, unchecked_return_type = "string")]
pub fn parse_json(input: &str) -> Result<String, JsValue> {
    let stmts = Parser::parse_all(input).map_err(qql_err_to_js)?;
    serde_json::to_string(&stmts).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = isValid)]
pub fn is_valid(input: &str) -> bool {
    // Full frontend gate: parse + plan — same contract as execution and the
    // language conformance suite.
    qql_plan::parse_and_plan(input).is_ok()
}

/// Inject a WHERE filter into a query string (`injectFilter` is the only
/// export — JS convention is camelCase).
#[wasm_bindgen(js_name = injectFilter)]
pub fn inject_filter(
    query: &str,
    field: &str,
    op: &str,
    value: JsValue,
) -> Result<JsValue, JsValue> {
    let val = super::params::jsvalue_to_value(&value).map_err(qql_err_to_js)?;
    let cmp = parse_comparison_op(op)?;
    let mut stmt = Parser::parse(query).map_err(qql_err_to_js)?;
    ast::inject_filter(&mut stmt, field, cmp, val).map_err(qql_err_to_js)?;
    to_js_value(&stmt)
}

pub(crate) fn parse_comparison_op(op: &str) -> Result<ComparisonOp, JsValue> {
    // Single source in qql-core: supported operators and rejection messages
    // stay identical across every SDK binding.
    qql_core::ast::ComparisonOp::parse_inject_op(op).map_err(qql_err_to_js)
}

// ── Core: tokenize ────────────────────────────────────────────────

#[wasm_bindgen]
pub fn tokenize(input: &str) -> Result<Vec<JsValue>, JsValue> {
    let lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(qql_err_to_js)?;
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

/// Largest-magnitude integers a JS `Number` holds exactly. Larger ones must
/// cross as `BigInt`; silently rounding them would mistarget points.
const MAX_SAFE_INTEGER: i64 = 9007199254740991;
const MIN_SAFE_INTEGER: i64 = -9007199254740991;

/// How one JSON number maps onto JS: exactly, or not at all.
#[derive(Debug, Clone, Copy, PartialEq)]
enum IntegerMapping {
    /// Fits a JS `Number` exactly — stays a number (full back-compat for
    /// counts, spans, limits, small IDs and payloads).
    Safe(f64),
    /// Unsigned snowflake (Qdrant u64 point IDs) — must be a `BigInt`.
    BigU(u64),
    /// Negative beyond the safe range — must be a `BigInt`.
    BigI(i64),
    /// Non-integer JSON number — a JS `Number` as always.
    Float(f64),
}

/// Classify one JSON number for the JS boundary. Pure over
/// `serde_json::Number`, so the boundary rule is unit-testable without a JS
/// runtime; the `JsValue` construction itself is a trivial follow-on.
fn classify_number(raw: &serde_json::Number) -> IntegerMapping {
    if let Some(signed) = raw.as_i64() {
        if (MIN_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&signed) {
            return IntegerMapping::Safe(signed as f64);
        }
        // Anything that fits `i64` but not the safe range keeps its sign.
        return IntegerMapping::BigI(signed);
    }
    if let Some(unsigned) = raw.as_u64() {
        // Reached only above `i64::MAX`: always beyond the safe range.
        return IntegerMapping::BigU(unsigned);
    }
    IntegerMapping::Float(raw.as_f64().unwrap_or(f64::NAN))
}

/// Map a JSON value onto JS with one rule: integers the JS `Number` type
/// holds exactly stay numbers; anything larger (Qdrant snowflake point IDs)
/// becomes a `BigInt` instead of failing closed (`json_compatible`) or
/// rounding silently. Single choke point for every JS-facing output:
/// execute, analyze, parse, compile, Stmt.
pub(crate) fn json_value_to_js(value: &serde_json::Value) -> JsValue {
    match value {
        serde_json::Value::Null => JsValue::NULL,
        serde_json::Value::Bool(flag) => JsValue::from_bool(*flag),
        serde_json::Value::Number(raw) => match classify_number(raw) {
            IntegerMapping::Safe(number) | IntegerMapping::Float(number) => {
                JsValue::from_f64(number)
            }
            IntegerMapping::BigU(unsigned) => js_sys::BigInt::from(unsigned).into(),
            IntegerMapping::BigI(signed) => js_sys::BigInt::from(signed).into(),
        },
        serde_json::Value::String(text) => JsValue::from_str(text),
        serde_json::Value::Array(items) => {
            let array = js_sys::Array::new_with_length(items.len() as u32);
            for (index, item) in items.iter().enumerate() {
                array.set(index as u32, json_value_to_js(item));
            }
            array.into()
        }
        serde_json::Value::Object(fields) => {
            let object = js_sys::Object::new();
            for (key, item) in fields {
                // Infallible on a fresh object with string keys; a host trap
                // here is unrecoverable, so there is no `QqlError` to carry.
                let _ =
                    js_sys::Reflect::set(&object, &JsValue::from_str(key), &json_value_to_js(item));
            }
            object.into()
        }
    }
}

pub(crate) fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    // Serialize through JSON first: `serde_json::Value` preserves full u64
    // precision from Rust, and `json_value_to_js` then applies the
    // safe-or-BigInt rule uniformly — typed structs and raw JSON alike, with
    // no per-field mapping to rot.
    let json = serde_json::to_value(val).map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(json_value_to_js(&json))
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
            let parsed = super::params::jsvalue_to_value(&p).map_err(qql_err_to_js)?;
            super::params::bind_value_params(query, &parsed, false)?
        }
        _ => query.to_string(),
    };
    let stmt = Parser::parse(&bound).map_err(qql_err_to_js)?;
    let compiled = routing::compile_statement(&stmt).map_err(qql_err_to_js)?;
    Ok(compiled_route_json(&compiled))
}

/// Compile one QQL statement into a JavaScript route object. Optional
/// `params` (object for `:name`, array for `?`) bind before parsing —
/// parity with `Client.compile(query, params)` on the Python and Node SDKs.
/// (`compileQuery` is the only module-level name — JS convention.)
#[wasm_bindgen(js_name = compileQuery, unchecked_return_type = "CompiledRoute")]
pub fn compile_query(query: &str, params: Option<JsValue>) -> Result<JsValue, JsValue> {
    let output = build_compile_output(query, params)?;
    to_js_value(&output)
}

/// Compiles QQL query into a safe, JS-owned Uint8Array byte buffer.
/// Optionally accepts `params` to bind before compiling.
#[wasm_bindgen(js_name = compileBytes)]
pub fn compile_bytes(query: &str, params: Option<JsValue>) -> Result<js_sys::Uint8Array, JsValue> {
    let output = build_compile_output(query, params)?;
    SCRATCH_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.clear();
        serde_json::to_writer(&mut *buf, &output).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(safe_owned_uint8_array(&buf))
    })
}

#[wasm_bindgen]
pub fn explain(query: &str) -> Result<String, JsValue> {
    qql_core::explain::explain(query).map_err(qql_err_to_js)
}

#[wasm_bindgen(js_name = explainBytes)]
pub fn explain_bytes(query: &str) -> Result<js_sys::Uint8Array, JsValue> {
    let exp_str = qql_core::explain::explain(query).map_err(qql_err_to_js)?;
    Ok(safe_owned_uint8_array(exp_str.as_bytes()))
}

/// Format a QQL string into canonical form.
#[wasm_bindgen(js_name = formatQuery)]
pub fn format_query(input: &str) -> Result<String, JsValue> {
    qql_core::fmt::format(input).map_err(qql_err_to_js)
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
            let parsed = super::params::jsvalue_to_value(&p).map_err(qql_err_to_js)?;
            super::params::bind_value_params(query, &parsed, truncate)
        }
        _ => Ok(query.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(json: serde_json::Value) -> IntegerMapping {
        let number = match &json {
            serde_json::Value::Number(raw) => raw.clone(),
            other => panic!("expected a JSON number, got {other}"),
        };
        classify_number(&number)
    }

    #[test]
    fn safe_integers_stay_numbers() {
        for (json, expected) in [
            (serde_json::json!(0), 0.0),
            (serde_json::json!(7), 7.0),
            (serde_json::json!(-3), -3.0),
            (serde_json::json!(9007199254740991i64), 9007199254740991.0),
            (serde_json::json!(-9007199254740991i64), -9007199254740991.0),
        ] {
            assert_eq!(classify(json), IntegerMapping::Safe(expected));
        }
    }

    #[test]
    fn snowflake_ids_map_to_bigint_exactly() {
        // Real geosmart hit: must survive the boundary without rounding.
        assert_eq!(
            classify(serde_json::json!(1479834607549681654u64)),
            IntegerMapping::BigI(1479834607549681654)
        );
        // Just past the safe range, both signs.
        assert_eq!(
            classify(serde_json::json!(9007199254740992u64)),
            IntegerMapping::BigI(9007199254740992)
        );
        assert_eq!(
            classify(serde_json::json!(-9007199254740992i64)),
            IntegerMapping::BigI(-9007199254740992)
        );
        // Above `i64::MAX` keeps the unsigned value.
        assert_eq!(
            classify(serde_json::Value::Number(serde_json::Number::from(
                u64::MAX
            ))),
            IntegerMapping::BigU(u64::MAX)
        );
    }

    #[test]
    fn floats_pass_through_untouched() {
        assert_eq!(
            classify(serde_json::json!(0.95)),
            IntegerMapping::Float(0.95)
        );
        assert_eq!(
            classify(serde_json::json!(1e21)),
            IntegerMapping::Float(1e21)
        );
    }
}
