//! Decode the remaining `QueryInterface` variants: order-by, sample, fusion,
//! RRF parameters, formula, and relevance feedback.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::interface::RrfParams;
use crate::decode::{formula, vector};
use crate::json::{self, child, index, invalid};
use qql_core::ast::{FeedbackItem, FeedbackStrategy, FusionMethod, QueryExpr};

/// Decode `{"order_by": field | {key, direction, start_from?}}`.
pub(crate) fn order_by(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    use qql_core::ast::OrderDirection;
    let ob_path = child(path, "order_by");
    let value = json::required(obj, "order_by", path)?;
    let (field, direction) = match value {
        Value::String(field) => (field.clone(), OrderDirection::Asc),
        Value::Object(ob) => {
            if ob.contains_key("start_from") {
                return Err(invalid(
                    child(&ob_path, "start_from"),
                    "ORDER BY start_from has no QQL representation",
                ));
            }
            let field = json::required(ob, "key", &ob_path)
                .and_then(|v| Ok(json::string_at(v, &child(&ob_path, "key"))?.to_string()))?;
            let direction = match ob.get("direction").filter(|v| !v.is_null()) {
                None => OrderDirection::Asc,
                Some(dir) => match json::string_at(dir, &child(&ob_path, "direction"))? {
                    "asc" => OrderDirection::Asc,
                    "desc" => OrderDirection::Desc,
                    other => {
                        return Err(invalid(
                            child(&ob_path, "direction"),
                            format!("unknown order direction '{other}'"),
                        ));
                    }
                },
            };
            (field, direction)
        }
        other => {
            return Err(invalid(
                &ob_path,
                format!(
                    "expected a field name or order object, got {}",
                    json::type_name(other)
                ),
            ));
        }
    };
    Ok(QueryExpr::OrderBy { field, direction })
}

/// Decode `{"sample": "random"}`.
pub(crate) fn sample(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let _ = obj;
    let sample = json::string_at(json::required(obj, "sample", path)?, &child(path, "sample"))?;
    if sample != "random" {
        return Err(invalid(
            child(path, "sample"),
            format!("unknown sample method '{sample}'"),
        ));
    }
    Ok(QueryExpr::SampleRandom)
}

/// Decode `{"fusion": "rrf" | "dbsf"}`.
pub(crate) fn fusion(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let method =
        match json::string_at(json::required(obj, "fusion", path)?, &child(path, "fusion"))? {
            "rrf" => FusionMethod::Rrf,
            "dbsf" => FusionMethod::Dbsf,
            other => {
                return Err(invalid(
                    child(path, "fusion"),
                    format!("unknown fusion method '{other}'"),
                ));
            }
        };
    Ok(QueryExpr::Fusion {
        method,
        prefetch: Vec::new(),
    })
}

/// Decode `{"rrf": {k?, weights?}}`.
pub(crate) fn rrf_params(obj: &json::Obj, path: &str) -> Result<RrfParams, ConvertError> {
    let rrf_path = child(path, "rrf");
    let rrf = json::object(json::required(obj, "rrf", path)?, &rrf_path)?;
    for key in rrf.keys() {
        if !matches!(key.as_str(), "k" | "weights") {
            return Err(invalid(child(&rrf_path, key), "unknown rrf parameter"));
        }
    }
    let weights = match rrf.get("weights").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(
            json::array(value, &child(&rrf_path, "weights"))?
                .iter()
                .enumerate()
                .map(|(i, w)| json::f64_at(w, &index(&child(&rrf_path, "weights"), i)))
                .collect::<Result<Vec<f64>, _>>()?,
        ),
    };
    Ok(RrfParams {
        k: json::opt_u64(rrf, "k", &rrf_path)?,
        weights,
    })
}

/// Decode `{"formula": Expression, "defaults": {…}}`.
pub(crate) fn formula_query(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let f_path = child(path, "formula");
    let expression = formula::formula(json::required(obj, "formula", path)?, &f_path)?;
    let defaults = match obj.get("defaults").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(value) => {
            let d_path = child(path, "defaults");
            json::object(value, &d_path)?
                .iter()
                .map(|(key, value)| {
                    Ok((
                        key.clone(),
                        json::json_to_ast_value(value, &child(&d_path, key))?,
                    ))
                })
                .collect::<Result<Vec<_>, ConvertError>>()?
        }
    };
    Ok(QueryExpr::Formula {
        expression: Box::new(expression),
        defaults,
        prefetch: Vec::new(),
    })
}

/// Decode `{"relevance_feedback": {target, feedback, strategy}}`.
pub(crate) fn relevance_feedback(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let rf_path = child(path, "relevance_feedback");
    let rf = json::object(json::required(obj, "relevance_feedback", path)?, &rf_path)?;
    let target = vector::query_input(
        json::required(rf, "target", &rf_path)?,
        &child(&rf_path, "target"),
    )?;
    let feedback_path = child(&rf_path, "feedback");
    let feedback = json::required(rf, "feedback", &rf_path)
        .and_then(|v| json::array(v, &feedback_path))?
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let item_path = index(&feedback_path, i);
            let item = json::object(item, &item_path)?;
            Ok(FeedbackItem {
                example: vector::query_input(
                    json::required(item, "example", &item_path)?,
                    &child(&item_path, "example"),
                )?,
                score: json::required(item, "score", &item_path)
                    .and_then(|v| json::f64_at(v, &child(&item_path, "score")))?,
            })
        })
        .collect::<Result<Vec<_>, ConvertError>>()?;
    if feedback.is_empty() {
        return Err(invalid(
            feedback_path,
            "QQL RELEVANCE FEEDBACK requires at least one feedback item",
        ));
    }
    let strategy_path = child(&rf_path, "strategy");
    let strategy = json::object(json::required(rf, "strategy", &rf_path)?, &strategy_path)?;
    let naive_path = child(&strategy_path, "naive");
    let naive = json::object(
        json::required(strategy, "naive", &strategy_path)?,
        &naive_path,
    )?;
    for key in naive.keys() {
        if !matches!(key.as_str(), "a" | "b" | "c") {
            return Err(invalid(
                child(&naive_path, key),
                "unknown naive strategy parameter",
            ));
        }
    }
    let weight = |name: &str| -> Result<f64, ConvertError> {
        json::required(naive, name, &naive_path)
            .and_then(|v| json::f64_at(v, &child(&naive_path, name)))
    };
    Ok(QueryExpr::RelevanceFeedback {
        target,
        feedback,
        strategy: FeedbackStrategy {
            a: weight("a")?,
            b: weight("b")?,
            c: weight("c")?,
        },
        using: None,
        prefetch: Vec::new(),
    })
}
