//! Bare-body operation detection by JSON structure.

use serde_json::Value;

use crate::ConvertError;
use crate::formulas::convert_formula_query;
use crate::operations::{
    convert_delete_by_filter, convert_delete_points, convert_discover, convert_get_points,
    convert_recommend, convert_scroll, convert_search, convert_set_payload, convert_upsert,
};
use crate::rest_types::{convert_create_collection, convert_create_index};

/// Detect the operation from a bare body's shape and convert it.
pub(crate) fn convert_by_structure(
    raw: &Value,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return Err(ConvertError::InvalidPayload("expected a JSON object")),
    };

    has_prefetch_and_query(obj, collection)
        .or_else(|| has_top_level_prefetch(obj, collection))
        .or_else(|| has_batch_search(obj, collection))
        .or_else(|| has_set_payload(obj, collection))
        .or_else(|| has_points_field(obj, collection))
        .or_else(|| has_vector_field(obj, collection))
        .or_else(|| has_positive_field(obj, collection))
        .or_else(|| has_target_field(obj, collection))
        .or_else(|| has_ids_field(obj, collection))
        .or_else(|| has_vectors_config(obj, collection))
        .or_else(|| has_field_name(obj, collection))
        .or_else(|| has_filter_field(obj, collection))
        .unwrap_or(Err(ConvertError::UndetectableOperation))
}

fn has_prefetch_and_query(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("query") && obj.get("query").and_then(|v| v.as_object()).is_some() {
        return Some(convert_formula_query(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_top_level_prefetch(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("prefetch") {
        return Some(convert_formula_query(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_batch_search(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("searches") {
        return Some(convert_formula_query(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_set_payload(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("payload") && (obj.contains_key("points") || obj.contains_key("filter")) {
        return Some(convert_set_payload(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_points_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if let Some(points) = obj.get("points").and_then(|v| v.as_array())
        && !points.is_empty()
    {
        if let Some(first) = points.first().and_then(|v| v.as_object())
            && (first.contains_key("vector") || first.contains_key("payload"))
        {
            return Some(convert_upsert(&Value::Object(obj.clone()), collection));
        }
        return Some(convert_delete_points(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_vector_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("vector") {
        return Some(convert_search(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_positive_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("positive") {
        return Some(convert_recommend(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_target_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("target") {
        return Some(convert_discover(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_ids_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("ids") {
        return Some(convert_get_points(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_vectors_config(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("vectors") || obj.contains_key("vectors_config") {
        return Some(convert_create_collection(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_field_name(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("field_name") {
        return Some(convert_create_index(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_filter_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, ConvertError>> {
    if obj.contains_key("filter") {
        if obj.contains_key("limit") {
            return Some(convert_scroll(&Value::Object(obj.clone()), collection));
        }
        return Some(convert_delete_by_filter(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}
