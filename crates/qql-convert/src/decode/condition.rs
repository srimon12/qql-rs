//! Decode field-scoped filter conditions (`FieldCondition`) into
//! [`qql_core::ast::FilterExpr`].

use crate::ConvertError;
use crate::decode::filter::filter;
use crate::decode::predicates::{decode_geo, decode_match, decode_range, decode_values_count};
use crate::json::{self, child, index, invalid};
use qql_core::ast::{FilterExpr, PointIdPredicate};

/// Decode `{key, match?, range?, geo_*, values_count?, is_empty?, is_null?}`.
pub(crate) fn field_condition(obj: &json::Obj, path: &str) -> Result<FilterExpr, ConvertError> {
    for key in obj.keys() {
        match key.as_str() {
            "key" | "match" | "range" | "geo_bounding_box" | "geo_radius" | "geo_polygon"
            | "values_count" | "is_empty" | "is_null" => {}
            other => {
                return Err(invalid(child(path, other), "unknown FieldCondition field"));
            }
        }
    }
    let field = field_name(obj, path)?;
    let mut predicates: Vec<FilterExpr> = Vec::new();

    if let Some(value) = obj.get("match").filter(|v| !v.is_null()) {
        predicates.push(decode_match(value, &child(path, "match"), &field)?);
    }
    if let Some(value) = obj.get("range").filter(|v| !v.is_null()) {
        predicates.push(decode_range(value, &child(path, "range"), &field)?);
    }
    for geo in ["geo_bounding_box", "geo_radius", "geo_polygon"] {
        if let Some(value) = obj.get(geo).filter(|v| !v.is_null()) {
            predicates.push(decode_geo(geo, value, &child(path, geo), &field)?);
        }
    }
    if let Some(value) = obj.get("values_count").filter(|v| !v.is_null()) {
        predicates.push(decode_values_count(
            value,
            &child(path, "values_count"),
            &field,
        )?);
    }
    for (key, predicate) in [
        (
            "is_empty",
            FilterExpr::IsEmpty {
                field: field.clone(),
            },
        ),
        (
            "is_null",
            FilterExpr::IsNull {
                field: field.clone(),
            },
        ),
    ] {
        if let Some(value) = obj.get(key).filter(|v| !v.is_null()) {
            let flag = json::bool_at(value, &child(path, key))?;
            predicates.push(if flag {
                predicate
            } else {
                FilterExpr::Not {
                    operand: Box::new(predicate),
                }
            });
        }
    }

    match predicates.len() {
        0 => Err(invalid(
            path,
            "field condition has no predicate (match/range/geo/values_count/is_empty/is_null)",
        )),
        1 => Ok(predicates.into_iter().next().expect("len checked")),
        _ => Ok(FilterExpr::And {
            operands: predicates,
        }),
    }
}

/// Required `key` string of a condition object.
pub(crate) fn field_name(obj: &json::Obj, path: &str) -> Result<String, ConvertError> {
    json::required(obj, "key", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "key"))?.to_string()))
}

/// Decode `{has_id: [id, …]}`.
pub(crate) fn has_id(obj: &json::Obj, path: &str) -> Result<FilterExpr, ConvertError> {
    let ids_path = child(path, "has_id");
    let ids = json::required(obj, "has_id", path)
        .and_then(|v| json::array(v, &ids_path))?
        .iter()
        .enumerate()
        .map(|(i, id)| crate::decode::vector::point_id(id, &index(&ids_path, i)))
        .collect::<Result<Vec<_>, _>>()?;
    match ids.len() {
        0 => Err(invalid(ids_path, "has_id must list at least one point id")),
        1 => Ok(FilterExpr::PointId(PointIdPredicate::Eq(
            ids.into_iter().next().expect("len checked"),
        ))),
        _ => Ok(FilterExpr::PointId(PointIdPredicate::In(ids))),
    }
}

/// Decode `{slice: {total, index}}`.
pub(crate) fn slice(obj: &json::Obj, path: &str) -> Result<FilterExpr, ConvertError> {
    let slice_path = child(path, "slice");
    let slice = json::required(obj, "slice", path).and_then(|v| json::object(v, &slice_path))?;
    let total = json::required(slice, "total", &slice_path)
        .and_then(|v| json::u64_at(v, &child(&slice_path, "total")))?;
    let index = json::required(slice, "index", &slice_path)
        .and_then(|v| json::u64_at(v, &child(&slice_path, "index")))?;
    if total == 0 || index >= total {
        return Err(invalid(
            slice_path,
            format!("invalid slice: total {total} must be >= 1 and index {index} < total"),
        ));
    }
    Ok(FilterExpr::Slice { total, index })
}

/// Decode `{nested: {key, filter}}`.
pub(crate) fn nested(obj: &json::Obj, path: &str) -> Result<FilterExpr, ConvertError> {
    let nested_path = child(path, "nested");
    let nested = json::required(obj, "nested", path).and_then(|v| json::object(v, &nested_path))?;
    let key = json::required(nested, "key", &nested_path)
        .and_then(|v| Ok(json::string_at(v, &child(&nested_path, "key"))?.to_string()))?;
    let filter = json::required(nested, "filter", &nested_path)
        .and_then(|v| filter(v, &child(&nested_path, "filter")))?;
    Ok(FilterExpr::Nested {
        path: key,
        filter: Box::new(filter),
    })
}
