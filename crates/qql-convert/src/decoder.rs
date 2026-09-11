//! Wrapped-request vs bare-body input decoding.

use serde_json::Value;

/// A parsed converter input: either a wrapped `{method, path, body}`
/// request or a bare REST body.
#[derive(Debug)]
pub(crate) enum DecodedInput<'a> {
    /// Wrapped request; `body` is `body` or `request`, when present.
    Wrapped {
        method: &'a str,
        path: &'a str,
        body: Option<&'a Value>,
    },
    /// Bare REST body without path context.
    Bare(&'a Value),
}

/// Split parsed JSON into a wrapped request or a bare body.
pub(crate) fn decode(raw: &Value) -> DecodedInput<'_> {
    if let Some(obj) = raw.as_object() {
        let method = obj.get("method").and_then(|v| v.as_str());
        let path = obj.get("path").and_then(|v| v.as_str());
        if let (Some(method), Some(path)) = (method, path) {
            let body = obj.get("body").or_else(|| obj.get("request"));
            return DecodedInput::Wrapped { method, path, body };
        }
    }
    DecodedInput::Bare(raw)
}
