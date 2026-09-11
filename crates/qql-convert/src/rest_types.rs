//! REST endpoint typing, collection extraction, and DDL converters.

use serde_json::Value;

use crate::ConvertError;
use crate::sanitize::sanitize_collection_name;

/// A wrapped request's `{method, path}` resolved to a known endpoint family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestEndpoint {
    /// `PUT /collections/{name}` (no `/points` or `/index` segment).
    CreateCollection,
    /// `DELETE /collections/{name}` (no `/points` segment).
    DeleteCollection,
    /// `PUT /collections/{name}/points`.
    Upsert,
    /// `POST /collections/{name}/points/query`.
    FormulaQuery,
    /// `POST /collections/{name}/points/search`.
    Search,
    /// `POST /collections/{name}/points/recommend`.
    Recommend,
    /// `POST /collections/{name}/points/discover`.
    Discover,
    /// `POST /collections/{name}/points/scroll`.
    Scroll,
    /// `POST /collections/{name}/points` (get points by IDs).
    GetPoints,
    /// `POST /collections/{name}/points/delete`.
    DeletePoints,
    /// `POST /collections/{name}/points/payload`.
    SetPayload,
    /// `PUT /collections/{name}/index`.
    CreateIndex,
}

/// Resolve `{method, path}` to an endpoint family.
///
/// `path` may carry a leading `/`.
pub(crate) fn parse_endpoint(method: &str, path: &str) -> Result<RestEndpoint, ConvertError> {
    let path = path.trim_start_matches('/');
    // Order matters: `/points/query` must not match the bare `/points` arm,
    // and create-collection must exclude `/points` and `/index` paths.
    let endpoint = if method == "PUT"
        && path.starts_with("collections/")
        && !path.contains("/points")
        && !path.contains("/index")
    {
        RestEndpoint::CreateCollection
    } else if method == "DELETE" && path.starts_with("collections/") && !path.contains("/points") {
        RestEndpoint::DeleteCollection
    } else if method == "PUT" && path.ends_with("/points") {
        RestEndpoint::Upsert
    } else if method == "POST" && path.ends_with("/points/query") {
        RestEndpoint::FormulaQuery
    } else if method == "POST" && path.ends_with("/points/search") {
        RestEndpoint::Search
    } else if method == "POST" && path.ends_with("/points/recommend") {
        RestEndpoint::Recommend
    } else if method == "POST" && path.ends_with("/points/discover") {
        RestEndpoint::Discover
    } else if method == "POST" && path.ends_with("/points/scroll") {
        RestEndpoint::Scroll
    } else if method == "POST" && path.ends_with("/points/delete") {
        // Must precede the bare `/points` arm.
        RestEndpoint::DeletePoints
    } else if method == "POST" && path.ends_with("/points/payload") {
        RestEndpoint::SetPayload
    } else if method == "POST"
        && path.ends_with("/points")
        && !path.contains("/search")
        && !path.contains("/recommend")
    {
        RestEndpoint::GetPoints
    } else if method == "PUT" && path.ends_with("/index") {
        RestEndpoint::CreateIndex
    } else {
        return Err(ConvertError::UnsupportedEndpoint(format!(
            "{method} {path}"
        )));
    };
    Ok(endpoint)
}

/// Extract the collection name from a REST path (`collections/{name}/...`).
pub(crate) fn extract_collection(path: &str) -> String {
    let path = path.trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 2 && parts[0] == "collections" {
        sanitize_collection_name(parts[1])
    } else {
        "unknown".to_string()
    }
}

/// Convert a create-collection body to `CREATE COLLECTION`.
pub(crate) fn convert_create_collection(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => {
            return Err(ConvertError::InvalidPayload(
                "invalid create collection JSON",
            ));
        }
    };

    let vectors = obj.get("vectors").or_else(|| obj.get("vectors_config"));

    let mut stmt = format!("CREATE COLLECTION {collection}");

    if let Some(Value::Object(map)) = vectors {
        let mut vec_defs = Vec::new();
        if map.contains_key("size") {
            vec_defs.push(build_vector_def("dense", map));
        } else {
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            for name in names {
                if let Some(vec_obj) = map[name].as_object() {
                    vec_defs.push(build_vector_def(name, vec_obj));
                }
            }
        }
        if !vec_defs.is_empty() {
            stmt.push_str(" (\n    ");
            stmt.push_str(&vec_defs.join(",\n    "));
            stmt.push_str("\n)");
        }
    }

    Ok(vec![stmt])
}

fn build_vector_def(name: &str, v: &serde_json::Map<String, Value>) -> String {
    let size = v
        .get("size")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "?".to_string());
    let distance = v
        .get("distance")
        .map(|d| d.to_string())
        .unwrap_or_else(|| "Cosine".to_string());
    let mut def = format!("'{name}' VECTOR({size}, {distance})");

    if let Some(mvc) = v.get("multivector_config").and_then(|v| v.as_object())
        && let Some(comp) = mvc.get("comparator")
    {
        def.push_str(&format!(" WITH MULTIVECTOR (comparator = '{comp}')"));
    }

    if let Some(hnsw) = v.get("hnsw_config").and_then(|v| v.as_object())
        && let Some(m) = hnsw.get("m")
    {
        def.push_str(&format!(" WITH HNSW (m = {m})"));
    }

    def
}

/// Convert a create-index body to `CREATE INDEX`.
pub(crate) fn convert_create_index(
    input: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("invalid create index JSON")),
    };

    let field = obj
        .get("field_name")
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "?".to_string());

    let schema = obj
        .get("field_schema")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Object(m) => m
                .get("type")
                .map(|t| format!("{t}"))
                .unwrap_or_else(|| "keyword".to_string()),
            _ => "keyword".to_string(),
        })
        .unwrap_or_else(|| "keyword".to_string());

    Ok(vec![format!(
        "CREATE INDEX ON COLLECTION {collection} FOR {field} TYPE {schema}"
    )])
}
