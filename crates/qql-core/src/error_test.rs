#[cfg(test)]
mod tests {
    use crate::error::QqlError;
    use alloc::string::ToString;

    #[test]
    fn test_syntax_error_message() {
        let err = QqlError::syntax("unexpected token", 42);
        let msg = err.to_string();
        assert!(msg.contains("42"), "error should contain position: {}", msg);
        assert!(
            msg.contains("unexpected token"),
            "error should contain message: {}",
            msg
        );
    }

    #[test]
    fn test_syntax_error_at_pos_0() {
        let err = QqlError::syntax("unexpected token", 0);
        let msg = err.to_string();
        assert!(
            !msg.contains("position"),
            "error at pos 0 should not show position: {}",
            msg
        );
        assert!(
            msg.contains("unexpected token"),
            "error should contain message: {}",
            msg
        );
    }

    #[test]
    fn test_runtime_error_message() {
        let err = QqlError::runtime("connection refused");
        let msg = err.to_string();
        assert!(
            msg.contains("runtime error"),
            "error should indicate runtime: {}",
            msg
        );
        assert!(
            msg.contains("connection refused"),
            "error should contain message: {}",
            msg
        );
    }

    #[test]
    fn test_syntax_error_matches_pos() {
        let err = QqlError::syntax("bad token", 5);
        assert_eq!(err.pos, 5);
    }

    #[test]
    fn test_runtime_error_pos_is_zero() {
        let err = QqlError::runtime("failure");
        assert_eq!(err.pos, 0);
    }

    #[test]
    fn test_error_clone_and_eq() {
        let err1 = QqlError::syntax("test", 10);
        let err2 = QqlError::syntax("test", 10);
        assert_eq!(err1, err2);

        let err3 = QqlError::syntax("test", 20);
        assert_ne!(err1, err3);

        let err4 = QqlError::runtime("test");
        let err5 = QqlError::runtime("test");
        assert_eq!(err4, err5);
    }

    #[test]
    fn test_syntax_with_pos() {
        let err = QqlError::syntax("unexpected token", 42);
        let msg = err.to_string();
        assert_eq!(msg, "syntax error at 42: unexpected token");
    }

    #[test]
    fn test_syntax_with_negative_pos_is_zero() {
        let err = QqlError::syntax("unexpected token", 0);
        let msg = err.to_string();
        assert_eq!(msg, "runtime error: unexpected token");
    }

    #[test]
    fn test_runtime_only_message() {
        let err = QqlError::runtime("connection refused");
        let msg = err.to_string();
        assert_eq!(msg, "runtime error: connection refused");
    }
}
