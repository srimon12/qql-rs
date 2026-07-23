use alloc::borrow::Cow;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ErrorKind {
    Lex,
    Parse,
    Validation,
    Execution,
    Transport,
    Backend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct QqlError {
    pub kind: ErrorKind,
    pub code: Cow<'static, str>,
    pub message: Cow<'static, str>,
    pub span: Option<Span>,
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
        }
    }

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
}

impl fmt::Display for QqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(span) = self.span {
            write!(f, " at {}..{}", span.start, span.end)?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for QqlError {}
