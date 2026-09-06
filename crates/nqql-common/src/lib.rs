//! Shared NAPI-RS logic for the `nqql` and `nqql-edge` bindings.
//!
//! The two Node SDKs expose different transports (REST/gRPC vs qdrant-edge)
//! but ship an identical parser/parameter surface. Every piece of that logic
//! lives here so the SDKs cannot drift: the crates keep only thin `#[napi]`
//! wrappers plus their transport-specific client construction, and the JS
//! wrapper keeps byte-identical copies of `dx-common.js` + `test_dx.js`
//! enforced by a CI diff check.
//!
//! Errors are returned as [`QqlError`] throughout; the SDK crates convert to
//! `napi::Error` at their boundary via [`to_napi_err`] / [`serde_napi_err`],
//! preserving the JSON payload the JS wrapper parses for `.code` / `.span`.

use qql_core::ast::{self, Value};
use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_plan::routing;

pub mod execute;

/// Serialize a [`QqlError`] to JSON so the JS wrapper can extract structured
/// fields (`code`, `kind`, `span`).
pub fn to_napi_err(e: QqlError) -> napi::Error {
    let json = serde_json::to_string(&e).unwrap_or_else(|_| e.to_string());
    napi::Error::from_reason(json)
}

/// Convert a serde_json error to a napi error.
pub fn serde_napi_err(e: serde_json::Error) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

// ═══════════════════════════════════════════════════════════════════
//  Stmt operations
// ═══════════════════════════════════════════════════════════════════

/// Parse a single QQL statement.
pub fn stmt_parse(input: &str) -> Result<ast::Stmt, QqlError> {
    Parser::parse(input)
}

/// Inject a WHERE filter into `stmt` in place. `op` is parsed by
/// [`ComparisonOp::parse_inject_op`] — the single source for supported
/// operators and rejection messages.
pub fn stmt_inject_filter(
    stmt: &mut ast::Stmt,
    field: &str,
    op: &str,
    value: serde_json::Value,
) -> Result<(), QqlError> {
    let cmp = qql_core::ast::ComparisonOp::parse_inject_op(op)?;
    let val = Value::from_json(value)?;
    ast::inject_filter(stmt, field, cmp, val)
}

/// Bind parameters into `stmt` and return the bound statement.
pub fn stmt_bind(
    stmt: &ast::Stmt,
    params: Option<&serde_json::Value>,
) -> Result<ast::Stmt, QqlError> {
    let mut inner = stmt.clone();
    if let Some(p) = params {
        qql_core::params_json::bind_stmt_with_params(&mut inner, p)?;
    }
    Ok(inner)
}

/// Canonical, re-parseable QQL (mirrors Python `str(stmt)`).
pub fn stmt_full(stmt: &ast::Stmt) -> String {
    qql_core::fmt::format_stmt(stmt)
}

/// Human-readable preview; long vectors are truncated (mirrors Python
/// `repr(stmt)`). May not re-parse.
pub fn stmt_readable(stmt: &ast::Stmt) -> String {
    qql_core::fmt::format_stmt_readable(stmt)
}

/// Compile a statement AST to its transport route, optionally binding
/// `params` first.
pub fn stmt_compile_route(
    stmt: &ast::Stmt,
    params: Option<&serde_json::Value>,
) -> Result<serde_json::Value, QqlError> {
    let bound = stmt_bind(stmt, params)?;
    let compiled = routing::compile_statement(&bound)?;
    let (method, path, payload) = match compiled.route {
        Some(route) => {
            let payload = route.body_json().unwrap_or(serde_json::Value::Null);
            (
                serde_json::Value::String(route.method.as_str().into()),
                serde_json::Value::String(route.path),
                payload,
            )
        }
        None => (
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
    };
    Ok(serde_json::json!({
        "stmt_type": compiled.stmt_type,
        "method": method,
        "path": path,
        "payload": payload,
    }))
}

// ═══════════════════════════════════════════════════════════════════
//  Parser surface
// ═══════════════════════════════════════════════════════════════════

/// Parse a script (one statement or semicolon-delimited) into AST statements.
pub fn parse_all(input: &str) -> Result<Vec<ast::Stmt>, QqlError> {
    Parser::parse_all(input)
}

/// Fast JSON-only parse — a JSON string of the AST array, bypassing V8 object
/// allocation entirely (~2× throughput). Ideal for HTTP/IPC forwarding.
pub fn parse_all_json(input: &str) -> Result<String, QqlError> {
    let stmts = Parser::parse_all(input)?;
    serde_json::to_string(&stmts)
        .map_err(|e| QqlError::execution("QQL-SERIALIZE-AST", e.to_string(), None))
}

/// Full frontend gate: parse + plan — the same contract as execution and the
/// language conformance suite.
pub fn is_valid(input: &str) -> bool {
    qql_plan::parse_and_plan(input).is_ok()
}

/// Inject a filter into a query string; returns the mutated AST as JSON.
pub fn inject_filter(
    query: &str,
    field: &str,
    op: &str,
    value: serde_json::Value,
) -> Result<serde_json::Value, QqlError> {
    let mut stmt = Parser::parse(query)?;
    stmt_inject_filter(&mut stmt, field, op, value)?;
    serde_json::to_value(&stmt)
        .map_err(|e| QqlError::execution("QQL-SERIALIZE-AST", e.to_string(), None))
}

/// Tokenize a query into `{ kind, text, pos, end, len }` views.
pub fn tokenize(input: &str) -> Result<serde_json::Value, QqlError> {
    #[derive(serde::Serialize)]
    struct TokenView<'a> {
        kind: &'a str,
        text: &'a str,
        pos: usize,
        end: usize,
        len: usize,
    }

    let lexer = Lexer::new(input);
    let mut tokens = Vec::with_capacity(input.len() / 4 + 1);
    for token_result in lexer {
        let token = token_result?;
        tokens.push(TokenView {
            kind: token.kind.as_str(),
            text: token.text,
            pos: token.span.start,
            end: token.span.end,
            len: token.span.end.saturating_sub(token.span.start),
        });
    }
    serde_json::to_value(&tokens)
        .map_err(|e| QqlError::execution("QQL-SERIALIZE-AST", e.to_string(), None))
}

/// Compile a QQL query to its transport route, optionally binding `params`.
pub fn compile_query(
    input: &str,
    params: Option<&serde_json::Value>,
) -> Result<serde_json::Value, QqlError> {
    let mut stmt = Parser::parse(input)?;
    if let Some(p) = params {
        qql_core::params_json::bind_stmt_with_params(&mut stmt, p)?;
    }
    stmt_compile_route(&stmt, None)
}

/// Tree-formatted plan explanation for a query string.
pub fn explain(query: &str) -> Result<String, QqlError> {
    qql_core::explain::explain(query)
}

/// Tree-formatted plan explanation for a parsed statement.
pub fn explain_stmt(stmt: &ast::Stmt) -> String {
    qql_core::explain::explain_node(stmt)
}
