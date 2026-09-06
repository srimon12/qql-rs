//! QQL language frontend: lexer, parser, typed AST, explain, and formatting.
//!
//! `qql-core` is the single owner of the QQL language surface. It performs no
//! I/O: no networking, no file access, and no knowledge of Qdrant REST JSON or
//! gRPC protobuf shapes. Lowering to transports lives in `qql-plan`
//! (transport-neutral plans plus an optional REST projection) and the executor
//! crate `qql` (REST / gRPC / edge execution).
//!
//! With default features the crate builds without third-party dependencies;
//! `serde` and `json` are opt-in for AST serialization and dynamic-value
//! conversion. Parser-only consumers — formatters, linters, language servers,
//! code generators — therefore embed it cheaply.
//!
//! # Entry points
//!
//! - [`parser::Parser`] — source text → typed [`Stmt`](ast::Stmt) AST with
//!   strict validation (`Parser::parse_all` for multi-statement scripts)
//! - [`ast::inject_filter`] — inject a typed comparison into an existing AST
//! - [`explain`] — tree-formatted statement dumps for humans
//! - [`fmt`] — canonical formatter; output re-parses to an identical AST
//! - [`params`] — `:name` / `?` placeholder binding and substitution
//!
//! # Errors
//!
//! Every failure is a structured [`error::QqlError`] carrying a stable `code`,
//! an explicit [`ErrorKind`](error::ErrorKind), and a byte-offset
//! [`Span`](error::Span).

extern crate alloc;

/// Typed abstract syntax tree: statements, filter and formula expressions,
/// values, and AST transforms.
pub mod ast;
/// Structured errors: stable codes, explicit [`ErrorKind`](error::ErrorKind),
/// byte-offset spans.
pub mod error;
/// Tree-formatted statement explanations.
pub mod explain;
/// Canonical QQL formatter with parse → format → parse round-trip guarantees.
pub mod fmt;
/// Byte-offset lexer: source text → [`Token`](token::Token) stream.
pub mod lexer;
/// Parameter binding for `:name` / `?` placeholders with type-checked
/// substitution.
pub mod params;
#[cfg(feature = "json")]
/// Host-language parameter binding over JSON-shaped `params` values: the
/// single batch-dispatch contract shared by every SDK binding.
pub mod params_json;
/// Recursive-descent parser: token stream → validated typed AST.
pub mod parser;
/// Lexical token kinds, spans, and keyword lookup tables.
pub mod token;

#[cfg(test)]
mod tests;
