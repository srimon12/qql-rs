//! Qdrant REST JSON to QQL statement converter.
//!
//! Five input shapes are accepted:
//!
//! 1. **Wrapped request** — `{"method", "path", "query"?, "body"?}`; the
//!    collection is derived from the path. `query` recovers `wait` /
//!    `timeout` / `consistency` (also accepted as a `?…` suffix on `path`).
//! 2. **Bare body** — raw Qdrant REST JSON without path context; the caller
//!    must supply the collection via [`convert`] / [`convert_stmts`].
//! 3. **JSONL capture** — one wrapped request or bare body per line, as
//!    written by `qql record`. A failing line reports its 1-based number via
//!    [`ConvertError::InvalidLine`].
//! 4. **HTTP snippet** — a `METHOD /path` line plus a JSON body, as shown in
//!    docs and blogs (full URLs and an `HTTP/x` suffix are accepted).
//! 5. **`curl` command** — a pasted invocation (`-X`, `-d`, `--header`, …,
//!    multi-line continuations, several commands). Shell-dynamic constructs
//!    (`$VAR`, `@file`, pipes) fail closed; see [`snippet`].
//!
//! Markdown fences around any of the above are stripped as paste noise.
//!
//! Conversion is contract-driven and AST-based:
//!
//! ```text
//! OpenAPI request JSON -> qql_core::ast::Stmt -> qql_core::fmt::format_stmt
//! ```
//!
//! The returned strings are **only** produced by the canonical formatter
//! (the same emitter behind `qql fmt`), so every emitted statement re-parses
//! and is byte-stable under `format_stmt(parse(emit))`. Decoding fails closed:
//! shapes the QQL AST cannot express return a typed [`ConvertError`], never a
//! placeholder or a silently dropped field.

mod bare;
mod convert;
mod decode;
mod endpoint;
mod json;
mod request;
mod shell;
mod snippet;

pub use convert::{convert, convert_stmts};
use std::fmt;

/// Typed conversion failure.
///
/// Returned by [`convert`] and [`convert_stmts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// The input is not valid JSON. Holds the underlying parse message.
    InvalidJson(String),
    /// A wrapped request targets an endpoint with no QQL mapping.
    /// Holds `"METHOD path"`.
    UnsupportedEndpoint(String),
    /// A bare body was given without a collection name.
    MissingCollection,
    /// The body is absent or structurally undecodable for the endpoint.
    /// Holds a human-readable reason.
    UndecodableBody {
        /// Why the body cannot be decoded.
        detail: String,
    },
    /// A structurally recognized body has an invalid or unrepresentable
    /// field at `path` (e.g. `points[0].vector`).
    InvalidField {
        /// Dot/bracket path of the offending field.
        path: String,
        /// Why the field is invalid or cannot be represented in QQL.
        detail: String,
    },
    /// A JSONL capture line failed to convert. Holds the 1-based line number
    /// and the underlying error.
    InvalidLine {
        /// 1-based line number of the failing capture entry.
        line: usize,
        /// The conversion error raised for that line.
        source: Box<ConvertError>,
    },
}

impl ConvertError {
    pub(crate) fn invalid(path: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::InvalidField {
            path: path.into(),
            detail: detail.into(),
        }
    }

    pub(crate) fn undecodable(detail: impl Into<String>) -> Self {
        Self::UndecodableBody {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(e) => write!(f, "invalid JSON: {e}"),
            Self::UnsupportedEndpoint(ep) => write!(f, "unsupported endpoint: {ep}"),
            Self::MissingCollection => write!(
                f,
                "bare request body requires a collection (pass --collection)"
            ),
            Self::UndecodableBody { detail } => write!(f, "cannot decode request body: {detail}"),
            Self::InvalidField { path, detail } => write!(f, "invalid field '{path}': {detail}"),
            Self::InvalidLine { line, source } => write!(f, "line {line}: {source}"),
        }
    }
}

impl std::error::Error for ConvertError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLine { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
