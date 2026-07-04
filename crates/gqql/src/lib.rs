use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[no_mangle]
pub extern "C" fn qql_parse(input: *const c_char) -> *mut c_char {
    let input_str = match cstr(input) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    match Parser::parse(input_str) {
        Ok(stmt) => to_c_string(&format!("{:#?}", stmt)),
        Err(e) => err(&e.to_string()),
    }
}

#[no_mangle]
pub extern "C" fn qql_parse_all(input: *const c_char) -> *mut c_char {
    let input_str = match cstr(input) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    match Parser::parse_all(input_str) {
        Ok(stmts) => {
            let list: Vec<String> = stmts.into_iter().map(|s| format!("{:#?}", s)).collect();
            match serde_json::to_string(&list) {
                Ok(json) => to_c_string(&json),
                Err(e) => err(&e.to_string()),
            }
        }
        Err(e) => err(&e.to_string()),
    }
}

#[no_mangle]
pub extern "C" fn qql_parse_batch(queries_json: *const c_char) -> *mut c_char {
    let json_str = match cstr(queries_json) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let queries: Vec<String> = match serde_json::from_str(json_str) {
        Ok(q) => q,
        Err(e) => return err(&format!("invalid input JSON: {}", e)),
    };
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        match Parser::parse(&q) {
            Ok(stmt) => results.push(format!("{:#?}", stmt)),
            Err(e) => return err(&e.to_string()),
        }
    }
    match serde_json::to_string(&results) {
        Ok(json) => to_c_string(&json),
        Err(e) => err(&e.to_string()),
    }
}

#[no_mangle]
pub extern "C" fn qql_is_valid(input: *const c_char) -> *mut c_char {
    let input_str = match cstr(input) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    match Parser::parse(input_str) {
        Ok(_) => to_c_string("true"),
        Err(_) => to_c_string("false"),
    }
}

#[no_mangle]
pub extern "C" fn qql_inject_filter(
    query: *const c_char,
    field: *const c_char,
    op: *const c_char,
    value_json: *const c_char,
) -> *mut c_char {
    let (query_str, field_str, op_str, value_str) =
        match (cstr(query), cstr(field), cstr(op), cstr(value_json)) {
            (Ok(q), Ok(f), Ok(o), Ok(v)) => (q, f, o, v),
            (Err(e), _, _, _) => return err(e),
            (_, Err(e), _, _) => return err(e),
            (_, _, Err(e), _) => return err(e),
            (_, _, _, Err(e)) => return err(e),
        };

    let value = match json_to_value(value_str) {
        Some(v) => v,
        None => return err("invalid value JSON"),
    };

    let mut stmt = match Parser::parse(query_str) {
        Ok(s) => s,
        Err(e) => return err(&e.to_string()),
    };

    ast::inject_filter(&mut stmt, field_str, op_str, &value);

    to_c_string(&format!("{:#?}", stmt))
}

#[no_mangle]
pub extern "C" fn qql_tokenize(input: *const c_char) -> *mut c_char {
    let input_str = match cstr(input) {
        Ok(s) => s,
        Err(e) => return err(e),
    };

    let lexer = Lexer::new(input_str);
    let mut tokens = Vec::new();

    for token_result in lexer {
        match token_result {
            Ok(tok) => {
                tokens.push(serde_json::json!({
                    "kind": tok.kind.as_str(),
                    "text": tok.text,
                    "pos": tok.pos,
                }));
            }
            Err(e) => return err(&e.to_string()),
        }
    }

    match serde_json::to_string(&tokens) {
        Ok(s) => to_c_string(&s),
        Err(_) => err("failed to serialize tokens"),
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn qql_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

// ── helpers ────────────────────────────────────────────────────────

fn cstr<'a>(ptr: *const c_char) -> Result<&'a str, &'static str> {
    if ptr.is_null() {
        return Err("null pointer");
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "invalid UTF-8")
}

fn err(msg: &str) -> *mut c_char {
    to_c_string(&format!("gqql error: {}", msg))
}

fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn json_to_value(json: &str) -> Option<Value<'static>> {
    let jv: serde_json::Value = serde_json::from_str(json).ok()?;
    serde_json_value_to_value(jv)
}

fn serde_json_value_to_value(jv: serde_json::Value) -> Option<Value<'static>> {
    match jv {
        serde_json::Value::String(s) => Some(Value::Str(Cow::Owned(s))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::Int(i))
            } else { n.as_f64().map(Value::Float) }
        }
        serde_json::Value::Bool(b) => Some(Value::Bool(b)),
        serde_json::Value::Null => Some(Value::Null),
        serde_json::Value::Array(items) => {
            let mut vals = Vec::with_capacity(items.len());
            for item in items {
                vals.push(serde_json_value_to_value(item)?);
            }
            Some(Value::List(vals))
        }
        serde_json::Value::Object(map) => {
            let mut pairs = Vec::with_capacity(map.len());
            for (k, v) in map {
                pairs.push((Cow::Owned(k), serde_json_value_to_value(v)?));
            }
            Some(Value::Dict(pairs))
        }
    }
}
