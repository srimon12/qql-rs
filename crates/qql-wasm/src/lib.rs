//! WebAssembly bindings for QQL: parse, plan, optional browser execute.
//!
//! Module layout: [`params`] (options + bind contracts), [`stmt`] (the `Stmt`
//! handle), [`functions`] (free parse/compile/bind entry points),
//! [`report`] (execution envelopes), [`response`] (REST response shaping),
//! [`client`] (browser transport + embedders), [`execute`] (execution entry
//! points and batching), [`embed`] (the `qql-embed` adapter).

#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod client;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod embed;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod execute;
mod functions;
mod params;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod pipeline;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod report;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod response;
mod stmt;

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub use client::Client;
pub use functions::{
    analyze, bind, compile, compile_bytes, compile_query, explain, explain_bytes, format_query,
    inject_filter, is_valid, parse, tokenize,
};
pub use stmt::Stmt;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const EXECUTION_TYPES: &str = r#"
export interface ExecuteOptions {
  onError?: "stop" | "continue";
  params?: Record<string, unknown> | unknown[];
}

export interface ExecResponse {
  ok: boolean;
  operation: string;
  message: string;
  data: unknown | null;
}

export interface ExecutionReport {
  ok: boolean;
  results: ExecResponse[];
  succeeded: number;
  failed: number;
}

export interface Token {
  kind: string;
  text: string;
  pos: number;
  end: number;
  len: number;
}

export interface CompiledRoute {
  stmt_type: string;
  method: string;
  path: string;
  payload: unknown | null;
}

export interface AnalysisError {
  code: string;
  message: string;
  start: number | null;
  end: number | null;
}

export interface AnalysisResult {
  valid: boolean;
  statements_count: number;
  tokens: Token[];
  ast: unknown[] | null;
  route: CompiledRoute | null;
  routes: CompiledRoute[];
  explain: string | null;
  error: AnalysisError | null;
}
"#;
