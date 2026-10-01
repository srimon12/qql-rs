//! Strict REST response parsing, re-exported from
//! [`qql_plan::surface::rest`].
//!
//! REST is the only transport that speaks JSON envelopes, so
//! `RestQdrant::execute_planned` is the canonical caller. Every function
//! reads exactly the OpenAPI response shape of its operation; a missing or
//! mistyped field fails with `QQL-BACKEND-ENVELOPE`. The parser lives in
//! `qql-plan::surface` so the WASM host shares it verbatim.

pub(crate) use qql_plan::surface::rest::*;
