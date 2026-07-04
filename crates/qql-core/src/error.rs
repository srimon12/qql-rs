use core::fmt;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QqlError {
    pub msg: Cow<'static, str>,
    pub pos: usize,
}

impl QqlError {
    pub fn syntax(msg: impl Into<Cow<'static, str>>, pos: usize) -> Self {
        QqlError {
            msg: msg.into(),
            pos,
        }
    }

    pub fn runtime(msg: impl Into<Cow<'static, str>>) -> Self {
        QqlError {
            msg: msg.into(),
            pos: 0,
        }
    }
}

impl fmt::Display for QqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.pos > 0 {
            write!(f, "syntax error at {}: {}", self.pos, self.msg)
        } else {
            write!(f, "runtime error: {}", self.msg)
        }
    }
}

impl std::error::Error for QqlError {}
