//! The `Stmt` handle: parsed statements with programmatic manipulation.

use qql_core::ast;

use super::functions::{SCRATCH_BUF, safe_owned_uint8_array};
use qql_core::error::QqlError;
use qql_core::parser::Parser;
use qql_plan::routing;
use wasm_bindgen::prelude::*;

use super::functions::{compiled_route_json, parse_comparison_op, to_js_value};

// ── Stmt class ─────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct Stmt {
    pub(crate) inner: qql_core::ast::Stmt,
    pub(crate) bound: bool,
}

#[wasm_bindgen]
impl Stmt {
    /// Parse a QQL string into a Stmt object for programmatic manipulation.
    #[wasm_bindgen(constructor)]
    pub fn new(input: &str) -> Result<Stmt, JsValue> {
        let inner = Parser::parse(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Stmt {
            inner,
            bound: false,
        })
    }

    /// Inject a WHERE filter into this statement's AST (mutates in place).
    #[wasm_bindgen(js_name = injectFilter)]
    pub fn inject_filter(&mut self, field: &str, op: &str, value: JsValue) -> Result<(), JsValue> {
        let val = super::params::jsvalue_to_value(&value)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let cmp = parse_comparison_op(op)?;
        ast::inject_filter(&mut self.inner, field, cmp, val)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(())
    }

    /// QQL `SHARD '…'` routing key (request-level). Prefer the clause in QQL.
    #[wasm_bindgen(getter, js_name = shardKey)]
    pub fn shard_key(&self) -> Option<String> {
        self.inner.shard_key().map(str::to_owned)
    }

    #[wasm_bindgen(setter, js_name = shardKey)]
    pub fn set_shard_key(&mut self, key: Option<String>) -> Result<(), JsValue> {
        if !self.inner.set_shard_key(key) {
            return Err(JsValue::from_str(
                "cannot set shardKey on statement type that does not support sharding (e.g. DDL statements)",
            ));
        }
        Ok(())
    }

    /// Whether parameters have already been bound into this statement.
    #[wasm_bindgen(getter, js_name = bound)]
    pub fn bound(&self) -> bool {
        self.bound
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

    /// Bind parameters into this statement and return a new bound Stmt.
    #[wasm_bindgen(js_name = bind)]
    pub fn bind(&self, params: Option<JsValue>) -> Result<Stmt, JsValue> {
        let binds_now = params
            .as_ref()
            .map(|p| !p.is_undefined() && !p.is_null())
            .unwrap_or(false);
        if binds_now && self.bound {
            return Err(JsValue::from_str(
                &serde_json::to_string(&QqlError::validation(
                    "QQL-BIND-ALREADY-BOUND",
                    "cannot bind parameters into a Stmt that has already been bound (params would be silently ignored)",
                    None,
                ))
                .unwrap_or_else(|_| "cannot bind parameters into an already bound Stmt".into()),
            ));
        }
        let mut stmt = self.inner.clone();
        if binds_now {
            let p = params.unwrap();
            let parsed = super::params::jsvalue_to_value(&p)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            super::params::bind_stmt_values(&mut stmt, &parsed)?;
        }
        Ok(Stmt {
            inner: stmt,
            bound: self.bound || binds_now,
        })
    }

    /// Format statement as canonical, re-parseable QQL (mirrors Python `str(stmt)`).
    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = toString)]
    pub fn to_string(&self) -> String {
        qql_core::fmt::format_stmt(&self.inner)
    }

    /// Format statement as a human-readable preview (mirrors Python `repr(stmt)`):
    /// long vector literals are truncated, so the output may not re-parse.
    #[wasm_bindgen(js_name = toReadableString)]
    pub fn to_readable_string(&self) -> String {
        qql_core::fmt::format_stmt_readable(&self.inner)
    }

    /// Compile this Stmt AST directly into a Qdrant REST route object.
    /// Optionally accepts `params` to bind before compiling.
    #[wasm_bindgen(js_name = compileRoute, unchecked_return_type = "CompiledRoute")]
    pub fn compile_route(&self, params: Option<JsValue>) -> Result<JsValue, JsValue> {
        let binds_now = params
            .as_ref()
            .map(|p| !p.is_undefined() && !p.is_null())
            .unwrap_or(false);
        if binds_now && self.bound {
            return Err(JsValue::from_str(
                &serde_json::to_string(&QqlError::validation(
                    "QQL-BIND-ALREADY-BOUND",
                    "cannot bind parameters into a Stmt that has already been bound (params would be silently ignored)",
                    None,
                ))
                .unwrap_or_else(|_| "cannot bind parameters into an already bound Stmt".into()),
            ));
        }
        let mut stmt = self.inner.clone();
        if binds_now {
            let p = params.unwrap();
            let parsed = super::params::jsvalue_to_value(&p)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            super::params::bind_stmt_values(&mut stmt, &parsed)?;
        }
        let compiled =
            routing::compile_statement(&stmt).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let output = compiled_route_json(&compiled);
        to_js_value(&output)
    }

    /// Compile this Stmt AST into a JS-owned Uint8Array byte buffer.
    #[wasm_bindgen(js_name = compileRouteBytes)]
    pub fn compile_route_bytes(&self) -> Result<js_sys::Uint8Array, JsValue> {
        let compiled = routing::compile_statement(&self.inner)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let output = compiled_route_json(&compiled);
        SCRATCH_BUF.with(|cell| {
            let mut buf = cell.borrow_mut();
            buf.clear();
            serde_json::to_writer(&mut *buf, &output)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(safe_owned_uint8_array(&buf))
        })
    }

    /// Explain this statement's execution plan (mirrors the free `explain`).
    #[wasm_bindgen(js_name = explain)]
    pub fn explain(&self) -> String {
        qql_core::explain::explain_node(&self.inner)
    }
}
