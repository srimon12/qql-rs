use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

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
    pub key: Cow<'static, str>,
    pub value: Cow<'static, str>,
}

impl ErrorField {
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
    pub kind: ErrorKind,
    pub code: Cow<'static, str>,
    pub message: Cow<'static, str>,
    pub span: Option<Span>,
    /// Structured key-value metadata providing machine-readable context
    /// (e.g. `collection`, `status_code`, `field_name`, `url`).
    pub fields: Vec<ErrorField>,
    /// Causal error that led to this one (error chaining).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub source: Option<Box<QqlError>>,
}

impl QqlError {
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

    pub fn lex(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Span,
    ) -> Self {
        Self::new(ErrorKind::Lex, code, message, Some(span))
    }

    pub fn parse(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Span,
    ) -> Self {
        Self::new(ErrorKind::Parse, code, message, Some(span))
    }

    pub fn validation(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Validation, code, message, span)
    }

    pub fn execution(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Execution, code, message, span)
    }

    pub fn transport(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Transport, code, message, span)
    }

    pub fn backend(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
        span: Option<Span>,
    ) -> Self {
        Self::new(ErrorKind::Backend, code, message, span)
    }

    pub(crate) fn syntax(message: impl Into<Cow<'static, str>>, position: usize) -> Self {
        Self::parse("QQL-PARSE-SYNTAX", message, Span::point(position))
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
        let err = QqlError::syntax("unexpected token", 0);
        let dyn_err: &dyn core::error::Error = &err;
        assert!(dyn_err.source().is_none());
        assert!(!dyn_err.to_string().is_empty());
    }
}
