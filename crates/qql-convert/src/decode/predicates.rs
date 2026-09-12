//! Decode `match`, `range`, geo, and `values_count` field predicates.

use serde_json::Value;

use crate::ConvertError;
use crate::json::{self, child, index, invalid};
use qql_core::ast::{ComparisonOp, FilterExpr, GeoPoint, Value as AstValue};

/// Decode a `Match` variant against a field.
pub(crate) fn decode_match(
    value: &Value,
    path: &str,
    field: &str,
) -> Result<FilterExpr, ConvertError> {
    let obj = json::object(value, path)?;
    let mut variant: Option<FilterExpr> = None;
    for key in obj.keys() {
        let expr = match key.as_str() {
            "value" => {
                let raw = json::required(obj, "value", path)?;
                let literal = match raw {
                    Value::String(_) | Value::Bool(_) => {
                        json::json_to_ast_value(raw, &child(path, "value"))?
                    }
                    Value::Number(n) if n.as_i64().is_some() => {
                        json::json_to_ast_value(raw, &child(path, "value"))?
                    }
                    other => {
                        return Err(invalid(
                            child(path, "value"),
                            format!(
                                "match value must be a string, integer, or boolean, got {}",
                                json::type_name(other)
                            ),
                        ));
                    }
                };
                FilterExpr::Compare {
                    field: field.to_string(),
                    op: ComparisonOp::Eq,
                    value: literal,
                }
            }
            "text" => FilterExpr::MatchText {
                field: field.to_string(),
                text: json::required(obj, "text", path)
                    .and_then(|v| Ok(json::string_at(v, &child(path, "text"))?.to_string()))?,
            },
            "phrase" => FilterExpr::MatchPhrase {
                field: field.to_string(),
                text: json::required(obj, "phrase", path)
                    .and_then(|v| Ok(json::string_at(v, &child(path, "phrase"))?.to_string()))?,
            },
            "prefix" => FilterExpr::MatchPrefix {
                field: field.to_string(),
                prefix: json::required(obj, "prefix", path)
                    .and_then(|v| Ok(json::string_at(v, &child(path, "prefix"))?.to_string()))?,
            },
            "any" => {
                let any_path = child(path, "any");
                let raw =
                    json::required(obj, "any", path).and_then(|v| json::array(v, &any_path))?;
                let values = raw
                    .iter()
                    .enumerate()
                    .map(|(i, item)| match item {
                        Value::String(_) | Value::Bool(_) => {
                            json::json_to_ast_value(item, &index(&any_path, i))
                        }
                        Value::Number(n) if n.as_i64().is_some() => {
                            json::json_to_ast_value(item, &index(&any_path, i))
                        }
                        other => Err(invalid(
                            index(&any_path, i),
                            format!(
                                "match any values must be strings or integers, got {}",
                                json::type_name(other)
                            ),
                        )),
                    })
                    .collect::<Result<Vec<AstValue>, _>>()?;
                if values.is_empty() {
                    // QQL `MATCH ANY` requires at least one literal; an empty
                    // upstream list matches no points and has no spelling.
                    return Err(invalid(any_path, "match any must list at least one value"));
                }
                FilterExpr::MatchAny {
                    field: field.to_string(),
                    values,
                }
            }
            // Legacy spellings still emitted by older Qdrant clients.
            "keyword" => FilterExpr::Compare {
                field: field.to_string(),
                op: ComparisonOp::Eq,
                value: AstValue::Str(
                    json::string_at(
                        json::required(obj, "keyword", path)?,
                        &child(path, "keyword"),
                    )?
                    .to_string(),
                ),
            },
            "integer" => FilterExpr::Compare {
                field: field.to_string(),
                op: ComparisonOp::Eq,
                value: AstValue::Int(
                    json::required(obj, "integer", path)
                        .and_then(|v| json::u64_at(v, &child(path, "integer")))?
                        .try_into()
                        .map_err(|_| {
                            invalid(child(path, "integer"), "integer exceeds the i64 range")
                        })?,
                ),
            },
            "boolean" => FilterExpr::Compare {
                field: field.to_string(),
                op: ComparisonOp::Eq,
                value: AstValue::Bool(
                    json::required(obj, "boolean", path)
                        .and_then(|v| json::bool_at(v, &child(path, "boolean")))?,
                ),
            },
            "text_any" => {
                return Err(invalid(
                    child(path, "text_any"),
                    "text_any (token-level match) has no QQL representation; use MATCH 'text' or MATCH ANY (values)",
                ));
            }
            "except" => {
                return Err(invalid(
                    child(path, "except"),
                    "match except (at-least-one-value-not-matching) has no QQL representation",
                ));
            }
            other => {
                return Err(invalid(child(path, other), "unknown Match variant"));
            }
        };
        if variant.replace(expr).is_some() {
            return Err(invalid(
                path,
                "match object must contain exactly one variant",
            ));
        }
    }
    variant.ok_or_else(|| invalid(path, "empty match object"))
}

/// Decode a `RangeInterface` (`{gt?, gte?, lt?, lte?}` of numbers or datetimes).
pub(crate) fn decode_range(
    value: &Value,
    path: &str,
    field: &str,
) -> Result<FilterExpr, ConvertError> {
    let obj = json::object(value, path)?;
    let mut typed: Vec<(ComparisonOp, AstValue)> = Vec::new();
    for key in obj.keys() {
        let op = match key.as_str() {
            "gt" => ComparisonOp::Gt,
            "gte" => ComparisonOp::Gte,
            "lt" => ComparisonOp::Lt,
            "lte" => ComparisonOp::Lte,
            other => {
                return Err(invalid(
                    child(path, other),
                    "unknown range bound (expected gt, gte, lt, lte)",
                ));
            }
        };
        let raw = obj.get(key).expect("key iterated");
        let literal = match raw {
            Value::Null => continue,
            Value::String(_) => {
                let s = json::string_at(raw, &child(path, key))?;
                if !qql_core::ast::looks_like_iso_datetime(s) {
                    return Err(invalid(
                        child(path, key),
                        "string range bounds must be ISO 8601 datetimes",
                    ));
                }
                AstValue::Str(s.to_string())
            }
            Value::Number(_) => {
                let n = json::f64_at(raw, &child(path, key))?;
                if let Some(i) = raw.as_i64() {
                    AstValue::Int(i)
                } else {
                    AstValue::Float(n)
                }
            }
            other => {
                return Err(invalid(
                    child(path, key),
                    format!(
                        "range bounds must be numbers or datetimes, got {}",
                        json::type_name(other)
                    ),
                ));
            }
        };
        typed.push((op, literal));
    }
    if typed.is_empty() {
        return Err(invalid(path, "range has no bounds"));
    }

    let find = |op: ComparisonOp| -> Option<AstValue> {
        typed.iter().find(|(o, _)| *o == op).map(|(_, v)| v.clone())
    };
    let (gte, lte) = (find(ComparisonOp::Gte), find(ComparisonOp::Lte));
    if let (Some(low), Some(high), true, true) = (
        gte,
        lte,
        find(ComparisonOp::Gt).is_none(),
        find(ComparisonOp::Lt).is_none(),
    ) {
        return Ok(FilterExpr::Between {
            field: field.to_string(),
            low,
            high,
        });
    }

    let comparisons: Vec<FilterExpr> = typed
        .into_iter()
        .map(|(op, value)| FilterExpr::Compare {
            field: field.to_string(),
            op,
            value,
        })
        .collect();
    Ok(match comparisons.len() {
        1 => comparisons.into_iter().next().expect("len checked"),
        _ => FilterExpr::And {
            operands: comparisons,
        },
    })
}

/// Decode a geo predicate (`geo_bounding_box` / `geo_radius` / `geo_polygon`).
pub(crate) fn decode_geo(
    name: &str,
    value: &Value,
    path: &str,
    field: &str,
) -> Result<FilterExpr, ConvertError> {
    match name {
        "geo_bounding_box" => {
            let obj = json::object(value, path)?;
            let top_left = geo_point(
                json::required(obj, "top_left", path)?,
                &child(path, "top_left"),
            )?;
            let bottom_right = geo_point(
                json::required(obj, "bottom_right", path)?,
                &child(path, "bottom_right"),
            )?;
            Ok(FilterExpr::GeoBoundingBox {
                field: field.to_string(),
                top_left,
                bottom_right,
            })
        }
        "geo_radius" => {
            let obj = json::object(value, path)?;
            let center = geo_point(json::required(obj, "center", path)?, &child(path, "center"))?;
            let radius = json::required(obj, "radius", path)
                .and_then(|v| json::f64_at(v, &child(path, "radius")))?;
            if radius <= 0.0 {
                return Err(invalid(
                    child(path, "radius"),
                    "geo radius must be greater than zero",
                ));
            }
            Ok(FilterExpr::GeoRadius {
                field: field.to_string(),
                center,
                radius,
            })
        }
        "geo_polygon" => {
            let obj = json::object(value, path)?;
            let exterior = geo_ring(
                json::required(obj, "exterior", path)?,
                &child(path, "exterior"),
            )?;
            let interiors = match obj.get("interiors").filter(|v| !v.is_null()) {
                None => Vec::new(),
                Some(rings) => {
                    let rings_path = child(path, "interiors");
                    json::array(rings, &rings_path)?
                        .iter()
                        .enumerate()
                        .map(|(i, ring)| geo_ring(ring, &index(&rings_path, i)))
                        .collect::<Result<Vec<_>, _>>()?
                }
            };
            Ok(FilterExpr::GeoPolygon {
                field: field.to_string(),
                exterior,
                interiors,
            })
        }
        other => Err(invalid(child(path, other), "unknown geo predicate")),
    }
}

/// Decode a `{lat, lon}` coordinate.
fn geo_point(value: &Value, path: &str) -> Result<GeoPoint, ConvertError> {
    let obj = json::object(value, path)?;
    let lat =
        json::required(obj, "lat", path).and_then(|v| json::f64_at(v, &child(path, "lat")))?;
    let lon =
        json::required(obj, "lon", path).and_then(|v| json::f64_at(v, &child(path, "lon")))?;
    Ok(GeoPoint { lat, lon })
}

/// Decode a `GeoLineString` (`{points: [{lat, lon}, …]}`).
///
/// The QQL parser requires at least three vertices per ring; fewer points
/// would produce an unparseable statement, so decoding fails closed first.
fn geo_ring(value: &Value, path: &str) -> Result<Vec<GeoPoint>, ConvertError> {
    let obj = json::object(value, path)?;
    let points_path = child(path, "points");
    let points = json::required(obj, "points", path)
        .and_then(|v| json::array(v, &points_path))?
        .iter()
        .enumerate()
        .map(|(i, point)| geo_point(point, &index(&points_path, i)))
        .collect::<Result<Vec<_>, _>>()?;
    if points.len() < 3 {
        return Err(invalid(
            points_path,
            "polygon rings must have at least 3 vertices",
        ));
    }
    Ok(points)
}

/// Decode a `ValuesCount` (`{gt?, gte?, lt?, lte?}` of unsigned integers).
pub(crate) fn decode_values_count(
    value: &Value,
    path: &str,
    field: &str,
) -> Result<FilterExpr, ConvertError> {
    let obj = json::object(value, path)?;
    let mut typed: Vec<(ComparisonOp, u64)> = Vec::new();
    for key in obj.keys() {
        let op = match key.as_str() {
            "gt" => ComparisonOp::Gt,
            "gte" => ComparisonOp::Gte,
            "lt" => ComparisonOp::Lt,
            "lte" => ComparisonOp::Lte,
            other => {
                return Err(invalid(
                    child(path, other),
                    "unknown values_count bound (expected gt, gte, lt, lte)",
                ));
            }
        };
        let raw = obj.get(key).expect("key iterated");
        if raw.is_null() {
            continue;
        }
        typed.push((op, json::u64_at(raw, &child(path, key))?));
    }
    if typed.is_empty() {
        return Err(invalid(path, "values_count has no bounds"));
    }
    let find = |op: ComparisonOp| typed.iter().find(|(o, _)| *o == op).map(|(_, v)| *v);
    if let (Some(gte), Some(lte)) = (find(ComparisonOp::Gte), find(ComparisonOp::Lte))
        && gte == lte
    {
        return Ok(FilterExpr::ValuesCount {
            field: field.to_string(),
            op: ComparisonOp::Eq,
            count: gte,
        });
    }
    let comparisons: Vec<FilterExpr> = typed
        .into_iter()
        .map(|(op, count)| FilterExpr::ValuesCount {
            field: field.to_string(),
            op,
            count,
        })
        .collect();
    Ok(match comparisons.len() {
        1 => comparisons.into_iter().next().expect("len checked"),
        _ => FilterExpr::And {
            operands: comparisons,
        },
    })
}
