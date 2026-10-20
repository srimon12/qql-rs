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
//!    fail closed unless resolved up front: [`ConvertOptions::vars`] expands
//!    placeholders before tokenizing, and [`ConvertOptions::read_file`]
//!    resolves `@file` bodies. Pipes and command substitution always fail;
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

pub use convert::{convert, convert_stmts, convert_stmts_with, convert_with};
use std::fmt;

/// Resolves a curl `@file` body from a path.
pub(crate) type ReadFile<'a> = Option<&'a dyn Fn(&str) -> Result<String, String>>;

/// Inputs beyond the text itself: placeholders to expand and file bodies to
/// read.
///
/// The crate stays I/O-free. [`Self::read_file`] is the injection point: the
/// `qql` CLI supplies a `std::fs::read_to_string` closure, tests supply an
/// in-memory map, and a caller that omits it keeps the fail-closed
/// `--data @file` error.
#[derive(Default)]
pub struct ConvertOptions<'a> {
    /// Values for `$NAME`, `${NAME}`, and `<NAME>`, expanded before parsing.
    /// The last entry for a name wins.
    pub vars: &'a [(String, String)],
    /// Resolves a curl `@file` body. Its error string is reported alongside
    /// the path that failed.
    #[allow(clippy::type_complexity)]
    pub read_file: Option<&'a dyn Fn(&str) -> Result<String, String>>,
}

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
    /// `$NAME` / `${NAME}` placeholders had no `--var` value. Holds every
    /// unresolved name, deduped and sorted.
    UnresolvedVariables {
        /// Names that were referenced but never supplied.
        names: Vec<String>,
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
            Self::UnresolvedVariables { names } => write!(
                f,
                "unresolved variables: {} (pass --var NAME=value)",
                names.join(", ")
            ),
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
