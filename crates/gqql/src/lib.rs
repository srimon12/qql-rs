use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[no_mangle]
pub extern "C" fn qql_parse(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return to_c_string("gqql error: null input");
    }

    let input_str = match unsafe { CStr::from_ptr(input) }.to_str() {
        Ok(s) => s,
        Err(e) => return to_c_string(&format!("gqql error: invalid UTF-8: {}", e)),
    };

    match Parser::parse(input_str) {
        Ok(stmt) => to_c_string(&format!("{:#?}", stmt)),
        Err(e) => to_c_string(&format!("gqql error: {}", e)),
    }
}

#[no_mangle]
pub extern "C" fn qql_tokenize(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return to_c_string("gqql error: null input");
    }

    let input_str = match unsafe { CStr::from_ptr(input) }.to_str() {
        Ok(s) => s,
        Err(e) => return to_c_string(&format!("gqql error: invalid UTF-8: {}", e)),
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
            Err(e) => return to_c_string(&format!("gqql error: {}", e)),
        }
    }

    match serde_json::to_string(&tokens) {
        Ok(s) => to_c_string(&s),
        Err(_) => to_c_string("gqql error: failed to serialize tokens"),
    }
}

#[no_mangle]
pub extern "C" fn qql_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
