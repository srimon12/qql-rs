use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

/// Source-code span as UTF-8 byte offsets into the query text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl Span {
    /// Create a span from explicit byte offsets.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Zero-length span at a single byte position.
    pub const fn point(position: usize) -> Self {
        Self::new(position, position)
    }
}

/// Broad category of error origin within the QQL pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ErrorKind {
    /// Lexer-level error (invalid token, unexpected character).
    Lex,
    /// Parser-level error (syntax error, unexpected token).
    Parse,
    /// Semantic validation error (invalid configuration, type mismatch).
    Validation,
    /// Execution-layer error (embedding failure, invariant violation).
    Execution,
    /// Transport-layer error (HTTP/gRPC connectivity, timeout).
    Transport,
    /// Qdrant backend error (non-success response, malformed response).
    Backend,
}

/// A key-value metadata field attached to a [`QqlError`] for structured context.
///
/// Use [`QqlError::with_field`] or the convenience builders
/// ([`QqlError::with_collection`], [`QqlError::with_status`], etc.) to attach
/// machine-readable context that clients can inspect without parsing the
/// human-readable message string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ErrorField {
    /// Field name, e.g. `collection` or `status_code`.
    pub key: Cow<'static, str>,
    /// Field value as a string.
    pub value: Cow<'static, str>,
}

impl ErrorField {
    /// Build an `ErrorField` from anything convertible into `Cow<'static, str>`.
    pub fn new(key: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Unified error type for the entire QQL pipeline.
///
/// Every error carries:
/// - a broad [`ErrorKind`] category,
/// - a machine-readable `code` (e.g. `QQL-EDGE-COLLECTION-NOT-FOUND`),
/// - a human-readable `message`,
/// - an optional source-code [`Span`],
/// - optional structured [`ErrorField`]s for machine-readable context, and
/// - an optional causal `source` error for chaining.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QqlError {
    /// Broad origin category within the pipeline.
    pub kind: ErrorKind,
    /// Machine-readable error code, e.g. `QQL-PARSE-SYNTAX`.
    pub code: Cow<'static, str>,
    /// Human-readable description of the failure.
    pub message: Cow<'static, str>,
    /// Optional source span in the original query text.
    pub span: Option<Span>,
    /// Structured key-value metadata providing machine-readable context
    /// (e.g. `collection`, `status_code`, `field_name`, `url`).
    pub fields: Vec<ErrorField>,
    /// Causal error that led to this one (error chaining).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub source: Option<Box<QqlError>>,
}

impl QqlError {
    /// Build an error with an explicit kind, code, message, and optional span.
    pub fn new(
        kind: ErrorKind,
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
            span,
            fields: Vec::new(),
            source: None,
        }
    }

    // ── Named constructors (keep existing API) ──────────────────────

    /// Create an error with `ErrorKind::Lex` and the given source span.
    pub fn lex(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Span,
    ) -> Self {
        Self::new(ErrorKind::Lex, code, message, Some(span))
    }

    /// Create an error with `ErrorKind::Parse` and the given source span.
    pub fn parse(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Span,
    ) -> Self {
        Self::new(ErrorKind::Parse, code, message, Some(span))
    }

    /// Create an error with `ErrorKind::Validation` and an optional span.
    pub fn validation(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Validation, code, message, span)
    }

    /// Create an error with `ErrorKind::Execution` and an optional span.
    pub fn execution(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Execution, code, message, span)
    }

    /// Create an error with `ErrorKind::Transport` and an optional span.
    pub fn transport(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Transport, code, message, span)
    }

    /// Create an error with `ErrorKind::Backend` and an optional span.
    pub fn backend(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Backend, code, message, span)
    }

    // ── Builder API ─────────────────────────────────────────────────

    /// Attach a structured key-value metadata field.
    ///
    /// ```
    /// # use qql_core::error::QqlError;
    /// let err = QqlError::backend("QQL-BACKEND", "unexpected status", None)
    ///     .with_field("status_code", "404")
    ///     .with_field("collection", "my_collection");
    /// assert_eq!(err.field("status_code"), Some("404"));
    /// assert_eq!(err.field("collection"), Some("my_collection"));
    /// ```
    pub fn with_field(
        mut self,
        key: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.fields.push(ErrorField::new(key, value));
        self
    }

    /// Shorthand for `.with_field("collection", name)`.
    pub fn with_collection(self, name: impl Into<Cow<'static, str>>) -> Self {
        self.with_field("collection", name)
    }

    /// Shorthand for `.with_field("status_code", code)`.
    pub fn with_status(self, code: u16) -> Self {
        self.with_field("status_code", alloc::format!("{code}"))
    }

    /// Shorthand for `.with_field("url", url)`.
    pub fn with_url(self, url: impl Into<Cow<'static, str>>) -> Self {
        self.with_field("url", url)
    }

    /// Shorthand for `.with_field("field_name", name)`.
    pub fn with_field_name(self, name: impl Into<Cow<'static, str>>) -> Self {
        self.with_field("field_name", name)
    }

    /// Shorthand for `.with_field("index_name", name)`.
    pub fn with_index_name(self, name: impl Into<Cow<'static, str>>) -> Self {
        self.with_field("index_name", name)
    }

    /// Shorthand for `.with_field("vector_name", name)`.
    pub fn with_vector_name(self, name: impl Into<Cow<'static, str>>) -> Self {
        self.with_field("vector_name", name)
    }

    /// Shorthand for `.with_field("model", name)`.
    pub fn with_model(self, name: impl Into<Cow<'static, str>>) -> Self {
        self.with_field("model", name)
    }

    /// Attach an optional source-code span.
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Chain a causal error.
    ///
    /// The `source` error will be displayed when the outer error is printed
    /// and is accessible via [`std::error::Error::source`].
    pub fn caused_by(mut self, source: QqlError) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Look up the first value for a given metadata key, case-insensitive.
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.key.eq_ignore_ascii_case(key))
            .map(|f| f.value.as_ref())
    }
}

/// Backend failure class shared by the CLI doctor, the WASM transport, and the
/// migrator's retry policy — one table instead of per-surface substring
/// heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendClass {
    /// Connection refusal, DNS failure, or timeout.
    Unreachable,
    /// Authentication / authorization failure.
    Auth,
    /// The target collection does not exist.
    CollectionNotFound,
    /// A search index is missing or not ready yet.
    IndexNotReady,
    /// Vector dimension mismatch.
    DimensionMismatch,
    /// Any other backend or HTTP failure.
    Http,
}

impl BackendClass {
    /// Classify an HTTP status plus response body/message.
    pub fn from_http(status: u16, message: &str) -> Self {
        let lower = message.to_ascii_lowercase();
        if status == 401
            || status == 403
            || lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("api key")
            || lower.contains("bearer")
        {
            Self::Auth
        } else if status == 404 || lower.contains("not found") {
            Self::CollectionNotFound
        } else if (lower.contains("index")
            && (lower.contains("not exist")
                || lower.contains("appropriate")
                || lower.contains("not ready")
                || lower.contains("missing")
                || lower.contains("indexing")
                || lower.contains("failed")))
            || lower.contains("no appropriate index")
        {
            Self::IndexNotReady
        } else if lower.contains("dimension")
            || lower.contains("vector size")
            || lower.contains("dimensions")
        {
            Self::DimensionMismatch
        } else {
            Self::Http
        }
    }

    /// Classify from an already-coded error (CLI doctor output).
    pub fn from_code(code: &str, message: &str) -> Self {
        if code == "QQL-TRANSPORT"
            || (code == "QQL-BACKEND-JSON" && message.contains("request id"))
            || message.contains("connection refused")
            || message.contains("Connection refused")
            || message.contains("dns error")
            || message.contains("failed to lookup address")
        {
            Self::Unreachable
        } else if code == "QQL-BACKEND-AUTH" {
            Self::Auth
        } else {
            Self::Http
        }
    }

    /// Machine-readable `QQL-BACKEND-*` code for this class.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unreachable => "QQL-TRANSPORT",
            Self::Auth => "QQL-BACKEND-AUTH",
            Self::CollectionNotFound => "QQL-BACKEND-COLLECTION-NOT-FOUND",
            Self::IndexNotReady => "QQL-BACKEND-INDEX-NOT-READY",
            Self::DimensionMismatch => "QQL-BACKEND-DIMENSION-MISMATCH",
            Self::Http => "QQL-BACKEND-HTTP",
        }
    }
}

/// HTTP statuses a retry can plausibly fix (rate limits, transient 5xx).
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

/// Whether a typed error is worth retrying: transport failures or a
/// retryable HTTP status recorded on the error.
pub fn is_retryable(err: &QqlError) -> bool {
    err.kind == ErrorKind::Transport
        || err
            .field("status_code")
            .and_then(|s| s.parse::<u16>().ok())
            .is_some_and(is_retryable_status)
}

/// Message-level retry fallback for errors that did not survive as
/// [`QqlError`] (e.g. executor report messages).
pub fn is_retryable_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("service unavailable")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("temporarily unavailable")
}

impl fmt::Display for QqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(span) = self.span {
            write!(f, " at {}..{}", span.start, span.end)?;
        }
        // Print structured fields when not empty
        if !self.fields.is_empty() {
            write!(f, " {{")?;
            for (i, field) in self.fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}: {}", field.key, field.value)?;
            }
            write!(f, "}}")?;
        }
        // Print causal chain
        if let Some(ref source) = self.source {
            write!(f, "\n  caused by: {source}")?;
        }
        Ok(())
    }
}

impl core::error::Error for QqlError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn core::error::Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_error_impl() {
        let err = QqlError::parse("QQL-PARSE-SYNTAX", "unexpected token", Span::point(0));
        let dyn_err: &dyn core::error::Error = &err;
        assert!(dyn_err.source().is_none());
        assert!(!dyn_err.to_string().is_empty());
    }

    #[test]
    fn backend_class_from_http_covers_every_code() {
        assert_eq!(BackendClass::from_http(401, "").code(), "QQL-BACKEND-AUTH");
        assert_eq!(BackendClass::from_http(403, "").code(), "QQL-BACKEND-AUTH");
        assert_eq!(
            BackendClass::from_http(400, "invalid api key").code(),
            "QQL-BACKEND-AUTH"
        );
        assert_eq!(
            BackendClass::from_http(404, "").code(),
            "QQL-BACKEND-COLLECTION-NOT-FOUND"
        );
        assert_eq!(
            BackendClass::from_http(500, "collection 'docs' not found").code(),
            "QQL-BACKEND-COLLECTION-NOT-FOUND"
        );
        assert_eq!(
            BackendClass::from_http(400, "vector size mismatch: expected 128, got 256").code(),
            "QQL-BACKEND-DIMENSION-MISMATCH"
        );
        assert_eq!(
            BackendClass::from_http(400, "no appropriate index for field").code(),
            "QQL-BACKEND-INDEX-NOT-READY"
        );
        assert_eq!(
            BackendClass::from_http(500, "internal server error").code(),
            "QQL-BACKEND-HTTP"
        );
    }

    #[test]
    fn backend_class_from_code_classifies_transport_and_auth() {
        assert_eq!(
            BackendClass::from_code("QQL-TRANSPORT", "connection refused"),
            BackendClass::Unreachable
        );
        assert_eq!(
            BackendClass::from_code("QQL-BACKEND-AUTH", "forbidden"),
            BackendClass::Auth
        );
        assert_eq!(
            BackendClass::from_code("QQL-BACKEND-HTTP", "boom"),
            BackendClass::Http
        );
    }

    #[test]
    fn retry_helpers_cover_transport_and_transient_statuses() {
        assert!(is_retryable(&QqlError::transport(
            "QQL-TRANSPORT",
            "reset",
            None
        )));
        assert!(is_retryable(
            &QqlError::backend("QQL-BACKEND-HTTP", "too many requests", None).with_status(429)
        ));
        assert!(!is_retryable(
            &QqlError::backend("QQL-BACKEND-AUTH", "forbidden", None).with_status(403)
        ));
        assert!(!is_retryable(
            &QqlError::backend("QQL-BACKEND-COLLECTION-NOT-FOUND", "missing", None)
                .with_status(404)
        ));
        assert!(is_retryable_message("HTTP 503 Service Unavailable"));
        assert!(!is_retryable_message("vector size mismatch"));
    }
}
