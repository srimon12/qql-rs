//! Decode the OpenAPI `Expression` schema into [`qql_core::ast::FormulaExpr`].

use serde_json::Value;

use crate::ConvertError;
use crate::json::{self, child, invalid};
use qql_core::ast::{ComparisonOp, FilterExpr, FormulaExpr};

/// Decode a formula `Expression` tree.
pub(crate) fn formula(value: &Value, path: &str) -> Result<FormulaExpr, ConvertError> {
    match value {
        Value::Number(_) => Ok(FormulaExpr::Constant {
            value: json::f64_at(value, path)?,
        }),
        Value::String(name) => Ok(FormulaExpr::Variable {
            // `$score` is the wire spelling of the reserved `score` variable.
            name: if name == "$score" {
                "score".to_string()
            } else {
                name.clone()
            },
        }),
        Value::Object(obj) => formula_object(obj, path),
        other => Err(invalid(
            path,
            format!(
                "expected a formula expression, got {}",
                json::type_name(other)
            ),
        )),
    }
}

/// Decode a formula expression object by its single variant key.
fn formula_object(obj: &json::Obj, path: &str) -> Result<FormulaExpr, ConvertError> {
    // A filter-shaped object is a boolean condition term.
    if obj.contains_key("key")
        || obj.contains_key("has_id")
        || obj.contains_key("has_vector")
        || obj.contains_key("slice")
        || obj.contains_key("nested")
        || obj.contains_key("is_empty")
        || obj.contains_key("is_null")
        || obj.contains_key("must")
        || obj.contains_key("should")
        || obj.contains_key("must_not")
    {
        return condition_term(obj, path);
    }

    if obj.len() != 1 {
        return Err(invalid(
            path,
            "formula expression objects must contain exactly one variant",
        ));
    }
    let (key, value) = obj.iter().next().expect("len checked");
    match key.as_str() {
        "sum" => Ok(fold_binary(value, &child(path, key), |left, right| {
            FormulaExpr::Sum { left, right }
        })?),
        "mult" => Ok(fold_binary(value, &child(path, key), |left, right| {
            FormulaExpr::Mul { left, right }
        })?),
        "max" => Ok(FormulaExpr::Max {
            args: args(value, &child(path, key))?,
        }),
        "min" => Ok(FormulaExpr::Min {
            args: args(value, &child(path, key))?,
        }),
        "neg" => Ok(FormulaExpr::Neg {
            operand: Box::new(formula(value, &child(path, key))?),
        }),
        "abs" => unary(value, path, key, |x| FormulaExpr::Abs { x }),
        "sqrt" => unary(value, path, key, |x| FormulaExpr::Sqrt { x }),
        "log10" => unary(value, path, key, |x| FormulaExpr::Log { x }),
        "ln" => unary(value, path, key, |x| FormulaExpr::Ln { x }),
        "exp" => unary(value, path, key, |x| FormulaExpr::Exp { x }),
        "acosh" => unary(value, path, key, |x| FormulaExpr::Acosh { x }),
        "pow" => {
            let params_path = child(path, key);
            let params = json::object(value, &params_path)?;
            let base = formula(
                json::required(params, "base", &params_path)?,
                &child(&params_path, "base"),
            )?;
            let exponent = formula(
                json::required(params, "exponent", &params_path)?,
                &child(&params_path, "exponent"),
            )?;
            let unused = params
                .keys()
                .filter(|k| k.as_str() != "base" && k.as_str() != "exponent")
                .count();
            if unused > 0 {
                return Err(invalid(params_path, "unknown pow parameter"));
            }
            Ok(FormulaExpr::Pow {
                base: Box::new(base),
                exponent: Box::new(exponent),
            })
        }
        "div" => {
            let params_path = child(path, key);
            let params = json::object(value, &params_path)?;
            for name in params.keys() {
                if !matches!(name.as_str(), "left" | "right" | "by_zero_default") {
                    return Err(invalid(child(&params_path, name), "unknown div parameter"));
                }
            }
            let left = formula(
                json::required(params, "left", &params_path)?,
                &child(&params_path, "left"),
            )?;
            let right = formula(
                json::required(params, "right", &params_path)?,
                &child(&params_path, "right"),
            )?;
            let by_zero_default = json::opt_f64(params, "by_zero_default", &params_path)?;
            Ok(FormulaExpr::Div {
                left: Box::new(left),
                right: Box::new(right),
                by_zero_default,
            })
        }
        "geo_distance" => {
            let params_path = child(path, key);
            let params = json::object(value, &params_path)?;
            let origin_path = child(&params_path, "origin");
            let origin = json::object(
                json::required(params, "origin", &params_path)?,
                &origin_path,
            )?;
            let lat = json::required(origin, "lat", &origin_path)
                .and_then(|v| json::f64_at(v, &child(&origin_path, "lat")))?;
            let lon = json::required(origin, "lon", &origin_path)
                .and_then(|v| json::f64_at(v, &child(&origin_path, "lon")))?;
            let field = json::required(params, "to", &params_path)
                .and_then(|v| Ok(json::string_at(v, &child(&params_path, "to"))?.to_string()))?;
            Ok(FormulaExpr::GeoDistance { lat, lon, field })
        }
        "exp_decay" | "lin_decay" | "gauss_decay" => decay(key, value, &child(path, key)),
        "datetime" => Ok(FormulaExpr::Datetime {
            value: json::string_at(value, &child(path, key))?.to_string(),
        }),
        "datetime_key" => Ok(FormulaExpr::DatetimeKey {
            key: json::string_at(value, &child(path, key))?.to_string(),
        }),
        other => Err(invalid(
            child(path, other),
            "unknown formula expression variant",
        )),
    }
}

/// Decode a required single-argument wrapper (`{"abs": …}`).
fn unary(
    value: &Value,
    path: &str,
    key: &str,
    wrap: impl FnOnce(Box<FormulaExpr>) -> FormulaExpr,
) -> Result<FormulaExpr, ConvertError> {
    Ok(wrap(Box::new(formula(value, &child(path, key))?)))
}

/// Decode a two-or-more element `sum` / `mult` array by left-folding.
///
/// The plan layer serializes these as exactly two elements (`Sub` becomes
/// `sum` of a `neg`), so folding preserves wire parity for every
/// plan-generated body while still accepting wider arrays.
fn fold_binary(
    value: &Value,
    path: &str,
    wrap: impl Fn(Box<FormulaExpr>, Box<FormulaExpr>) -> FormulaExpr,
) -> Result<FormulaExpr, ConvertError> {
    let items = args(value, path)?;
    let mut iter = items.into_iter();
    let first = iter.next().expect("args rejects empty arrays");
    Ok(iter.fold(first, |left, right| wrap(Box::new(left), Box::new(right))))
}

/// Decode a non-empty expression array.
fn args(value: &Value, path: &str) -> Result<Vec<FormulaExpr>, ConvertError> {
    let items = json::array(value, path)?;
    if items.is_empty() {
        return Err(invalid(path, "expression list must not be empty"));
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| formula(item, &crate::json::index(path, i)))
        .collect()
}

/// Decode a `{x, target?, scale?, midpoint?}` decay curve.
fn decay(key: &str, value: &Value, path: &str) -> Result<FormulaExpr, ConvertError> {
    let params = json::object(value, path)?;
    for name in params.keys() {
        if !matches!(name.as_str(), "x" | "target" | "scale" | "midpoint") {
            return Err(invalid(child(path, name), "unknown decay parameter"));
        }
    }
    let x = formula(json::required(params, "x", path)?, &child(path, "x"))?;
    let target = match params.get("target").filter(|v| !v.is_null()) {
        None => None,
        Some(target) => Some(Box::new(formula(target, &child(path, "target"))?)),
    };
    Ok(FormulaExpr::Decay {
        kind: key.to_string(),
        x: Box::new(x),
        target,
        scale: json::opt_f64(params, "scale", path)?,
        midpoint: json::opt_f64(params, "midpoint", path)?,
    })
}

/// Decode a filter-shaped object used as a formula term.
///
/// Only shapes the AST can lower back to the same condition are accepted:
/// a single `field = value` comparison (`MATCH(field, value)`) or a match-any
/// list (`MATCH(field, [values])`). Compound conditions have no standalone
/// `FormulaExpr` term and fail closed.
fn condition_term(obj: &json::Obj, path: &str) -> Result<FormulaExpr, ConvertError> {
    let decoded = crate::decode::filter::condition(&Value::Object(obj.clone()), path)?;
    match decoded {
        FilterExpr::Compare {
            field,
            op: ComparisonOp::Eq,
            value,
        } => Ok(FormulaExpr::MatchCondition {
            field,
            values: vec![value],
        }),
        FilterExpr::MatchAny { field, values } => Ok(FormulaExpr::MatchCondition { field, values }),
        _ => Err(invalid(
            path,
            "compound conditions have no standalone formula term; only MATCH(field, value) / MATCH(field, [values]) are supported",
        )),
    }
}
