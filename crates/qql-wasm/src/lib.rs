//! WebAssembly bindings for QQL: parse, plan, optional browser execute.
//!
//! Module layout: [`params`] (options + bind contracts), [`stmt`] (the `Stmt`
//! handle), [`functions`] (free parse/compile/bind entry points),
//! [`report`] (execution envelopes), [`response`] (strict REST response
//! shaping), [`schema`] (collection metadata/schema shaping), [`telemetry`]
//! (server time and usage), [`client`] (browser transport + embedders),
//! [`execute`] (execution entry points and batching), [`analyze`] (execution
//! profiling), [`embed`] (the `qql-embed` adapter).
//!
//! `report`, `response`, `schema`, and `telemetry` are pure JSON shaping with
//! no transport or `wasm-bindgen` dependency, so they also compile on the host
//! under `cargo test -p qql-wasm` for unit coverage.

#[cfg(all(feature = "client", target_arch = "wasm32"))]
mod analyze;
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
#[cfg(any(test, all(feature = "client", target_arch = "wasm32")))]
mod report;
#[cfg(any(test, all(feature = "client", target_arch = "wasm32")))]
mod response;
#[cfg(any(test, all(feature = "client", target_arch = "wasm32")))]
mod schema;
mod stmt;
#[cfg(any(test, all(feature = "client", target_arch = "wasm32")))]
mod telemetry;

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

export interface ServerTelemetry {
  time_s?: number | null;
  usage?: ServerUsage | null;
}

export interface ServerUsage {
  hardware?: HardwareUsage | null;
  inference?: InferenceUsage | null;
}

export interface HardwareUsage {
  cpu: number;
  payload_io_read: number;
  payload_io_write: number;
  payload_index_io_read: number;
  payload_index_io_write: number;
  vector_io_read: number;
  vector_io_write: number;
}

export interface InferenceUsage {
  models: Record<string, { tokens: number }>;
}

export interface PhaseTimings {
  parse_ms: number;
  prepare_plan_ms: number;
  dispatch_ms: number;
  decode_ms: number;
  total_ms: number;
}

export interface ExecResponse {
  ok: boolean;
  operation: string;
  message: string;
  data: unknown | null;
  telemetry?: ServerTelemetry | null;
}

export interface ExecutionReport {
  ok: boolean;
  results: ExecResponse[];
  succeeded: number;
  failed: number;
  telemetry?: ServerTelemetry | null;
  hits(stmt?: number): Array<Record<string, unknown>>;
  points(stmt?: number): Array<Record<string, unknown>>;
  ids(stmt?: number): Array<string | number>;
  facet(stmt?: number): Array<{ value: unknown; count: number }>;
  count(stmt?: number): number;
  groups(stmt?: number): Array<{ id: unknown; hits: Array<Record<string, unknown>> }>;
}

export interface AnalyzeReport {
  ok: boolean;
  plan: string;
  phases: PhaseTimings;
  server_time_s: number | null;
  usage: ServerUsage | null;
  results: ExecResponse[];
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
  /** First error, kept for older IDE clients. Prefer `errors`. */
  error: AnalysisError | null;
  /** Every recoverable diagnostic from panic-mode parse + plan. */
  errors: AnalysisError[];
}
"#;
