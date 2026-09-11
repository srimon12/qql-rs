//! Public REST JSON to QQL entry points plus wrapped-endpoint dispatch.

use serde_json::Value;

use crate::ConvertError;
use crate::decoder::{DecodedInput, decode};
use crate::detect::convert_by_structure;
use crate::formulas::convert_formula_query;
use crate::operations::{
    convert_delete_points, convert_discover, convert_get_points, convert_recommend, convert_scroll,
    convert_search, convert_set_payload, convert_upsert,
};
use crate::rest_types::{
    RestEndpoint, convert_create_collection, convert_create_index, extract_collection,
    parse_endpoint,
};
use crate::sanitize::sanitize_collection_name;

/// Convert a Qdrant REST JSON payload to QQL statements.
///
/// Accepts a wrapped `{method, path, body}` request (collection derived from
/// the path) or a bare body (collection falls back to `"unknown"`; prefer
/// [`json_to_qql_with_collection`] when the collection is known).
pub fn json_to_qql(input: &str) -> Result<Vec<String>, ConvertError> {
    json_to_qql_with_collection(input, "unknown")
}

/// Convert a Qdrant REST JSON payload to QQL statements.
///
/// `collection` is used when the payload carries no path information (bare
/// body). Wrapped requests always derive the collection from their path.
/// An empty collection falls back to `"unknown"`.
pub fn json_to_qql_with_collection(
    input: &str,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let input = input.trim();
    let collection = if collection.is_empty() {
        "unknown"
    } else {
        collection
    };
    let collection = sanitize_collection_name(collection);

    let raw: Value =
        serde_json::from_str(input).map_err(|e| ConvertError::InvalidJson(e.to_string()))?;

    match decode(&raw) {
        DecodedInput::Wrapped { method, path, body } => convert_by_endpoint(method, path, body),
        DecodedInput::Bare(raw) => convert_by_structure(raw, &collection),
    }
}

/// Dispatch a wrapped `{method, path, body}` request to its converter.
///
/// The collection always comes from the path; `body` is `body`/`request`
/// when present, otherwise null.
fn convert_by_endpoint(
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Vec<String>, ConvertError> {
    const NULL: Value = Value::Null;
    let endpoint = parse_endpoint(method, path)?;
    if endpoint == RestEndpoint::FormulaQuery && body.is_none() {
        return Ok(vec![]);
    }
    let body = body.unwrap_or(&NULL);
    let collection = extract_collection(path);
    match endpoint {
        RestEndpoint::CreateCollection => convert_create_collection(body, &collection),
        RestEndpoint::DeleteCollection => Ok(vec![format!("DROP COLLECTION {collection}")]),
        RestEndpoint::Upsert => convert_upsert(body, &collection),
        RestEndpoint::FormulaQuery => convert_formula_query(body, &collection),
        RestEndpoint::Search => convert_search(body, &collection),
        RestEndpoint::Recommend => convert_recommend(body, &collection),
        RestEndpoint::Discover => convert_discover(body, &collection),
        RestEndpoint::Scroll => convert_scroll(body, &collection),
        RestEndpoint::GetPoints => convert_get_points(body, &collection),
        RestEndpoint::DeletePoints => convert_delete_points(body, &collection),
        RestEndpoint::SetPayload => convert_set_payload(body, &collection),
        RestEndpoint::CreateIndex => convert_create_index(body, &collection),
    }
}
