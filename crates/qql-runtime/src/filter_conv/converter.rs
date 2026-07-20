use std::string::String;
use std::vec::Vec;

use qql_core::ast::{FilterExpr, Value};
use qql_core::error::QqlError;

pub struct FilterConverter;

impl Default for FilterConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterConverter {
    pub fn new() -> Self {
        FilterConverter
    }

    pub fn build_filter(
        &self,
        expr: &FilterExpr,
    ) -> Result<Option<crate::backend::Filter>, QqlError> {
        let condition = self.build_condition(expr)?;
        let filter_val = self.wrap_as_filter(condition);
        Ok(Some(crate::backend::Filter::from_json(filter_val)))
    }

    fn build_condition(&self, expr: &FilterExpr) -> Result<serde_json::Value, QqlError> {
        match expr {
            FilterExpr::Compare { field, op, value } => self.build_compare_expr(field, op, value),
            FilterExpr::Between { field, low, high } => self.build_between_expr(field, low, high),
            FilterExpr::In { field, values } => self.build_in_expr(field, values),
            FilterExpr::NotIn { field, values } => self.build_not_in_expr(field, values),
            FilterExpr::IsNull { field } => Ok(serde_json::json!({
                "is_null": { "key": field }
            })),
            FilterExpr::IsNotNull { field } => Ok(serde_json::json!({
                "must_not": [
                    { "is_null": { "key": field } }
                ]
            })),
            FilterExpr::IsEmpty { field } => Ok(serde_json::json!({
                "is_empty": { "key": field }
            })),
            FilterExpr::IsNotEmpty { field } => Ok(serde_json::json!({
                "must_not": [
                    { "is_empty": { "key": field } }
                ]
            })),
            FilterExpr::MatchText { field, text } => Ok(serde_json::json!({
                "key": field,
                "match": { "text": text }
            })),
            FilterExpr::MatchAny { field, text } => Ok(serde_json::json!({
                "key": field,
                "match": { "any": [text] }
            })),
            FilterExpr::MatchPhrase { field, text } => Ok(serde_json::json!({
                "key": field,
                "match": { "phrase": text }
            })),
            FilterExpr::And { operands } => self.build_and_expr(operands),
            FilterExpr::Or { operands } => self.build_or_expr(operands),
            FilterExpr::Not { operand } => self.build_not_expr(operand),
            FilterExpr::Nested { path, filter } => self.build_nested_expr(path, filter),
            FilterExpr::HasVector { name } => Ok(serde_json::json!({
                "has_vector": name
            })),
            FilterExpr::ValuesCount { field, op, count } => {
                let mut range = serde_json::json!({});
                match *op {
                    ">" => range["gt"] = serde_json::json!(count),
                    ">=" => range["gte"] = serde_json::json!(count),
                    "<" => range["lt"] = serde_json::json!(count),
                    "<=" => range["lte"] = serde_json::json!(count),
                    "=" => {
                        range["gte"] = serde_json::json!(count);
                        range["lte"] = serde_json::json!(count);
                    }
                    _ => {
                        return Err(QqlError::runtime(format!(
                            "unsupported values_count operator: {}",
                            op
                        )))
                    }
                }
                Ok(serde_json::json!({
                    "key": field,
                    "values_count": range
                }))
            }
            FilterExpr::GeoBoundingBox {
                field,
                top_left_lat,
                top_left_lon,
                bottom_right_lat,
                bottom_right_lon,
            } => Ok(serde_json::json!({
                "key": field,
                "geo_bounding_box": {
                    "top_left": {
                        "lat": top_left_lat,
                        "lon": top_left_lon
                    },
                    "bottom_right": {
                        "lat": bottom_right_lat,
                        "lon": bottom_right_lon
                    }
                }
            })),
            FilterExpr::GeoRadius {
                field,
                lat,
                lon,
                radius,
            } => Ok(serde_json::json!({
                "key": field,
                "geo_radius": {
                    "center": {
                        "lat": lat,
                        "lon": lon
                    },
                    "radius": radius
                }
            })),
        }
    }

    fn build_compare_expr(
        &self,
        field: &str,
        op: &str,
        value: &Value,
    ) -> Result<serde_json::Value, QqlError> {
        match op {
            "=" => self.build_equal_condition(field, value),
            "!=" => self.build_not_equal_condition(field, value),
            ">" => {
                let v = to_float64(value)?;
                Ok(serde_json::json!({
                    "key": field,
                    "range": { "gt": v }
                }))
            }
            ">=" => {
                let v = to_float64(value)?;
                Ok(serde_json::json!({
                    "key": field,
                    "range": { "gte": v }
                }))
            }
            "<" => {
                let v = to_float64(value)?;
                Ok(serde_json::json!({
                    "key": field,
                    "range": { "lt": v }
                }))
            }
            "<=" => {
                let v = to_float64(value)?;
                Ok(serde_json::json!({
                    "key": field,
                    "range": { "lte": v }
                }))
            }
            _ => Err(QqlError::runtime(format!(
                "unknown comparison operator: {}",
                op
            ))),
        }
    }

    fn build_between_expr(
        &self,
        field: &str,
        low: &Value,
        high: &Value,
    ) -> Result<serde_json::Value, QqlError> {
        let low_v = to_float64(low)?;
        let high_v = to_float64(high)?;
        Ok(serde_json::json!({
            "key": field,
            "range": {
                "gte": low_v,
                "lte": high_v
            }
        }))
    }

    fn build_in_expr(&self, field: &str, values: &[Value]) -> Result<serde_json::Value, QqlError> {
        self.build_set_condition(field, values, false)
    }

    fn build_not_in_expr(
        &self,
        field: &str,
        values: &[Value],
    ) -> Result<serde_json::Value, QqlError> {
        self.build_set_condition(field, values, true)
    }

    fn build_and_expr(&self, operands: &[FilterExpr]) -> Result<serde_json::Value, QqlError> {
        let mut must = Vec::with_capacity(operands.len());
        for operand in operands {
            let cond = self.build_condition(operand)?;
            must.push(cond);
        }
        Ok(serde_json::json!({
            "must": must
        }))
    }

    fn build_or_expr(&self, operands: &[FilterExpr]) -> Result<serde_json::Value, QqlError> {
        let mut should = Vec::with_capacity(operands.len());
        for operand in operands {
            let cond = self.build_condition(operand)?;
            should.push(cond);
        }
        Ok(serde_json::json!({
            "should": should
        }))
    }

    fn build_not_expr(&self, operand: &FilterExpr) -> Result<serde_json::Value, QqlError> {
        let cond = self.build_condition(operand)?;
        Ok(serde_json::json!({
            "must_not": [cond]
        }))
    }

    fn build_nested_expr(
        &self,
        path: &str,
        filter: &FilterExpr,
    ) -> Result<serde_json::Value, QqlError> {
        let inner = self.build_filter(filter)?;
        match inner {
            Some(f) => Ok(serde_json::json!({
                "nested": {
                    "key": path,
                    "filter": f
                }
            })),
            None => Err(QqlError::runtime("empty nested filter")),
        }
    }

    fn build_equal_condition(
        &self,
        field: &str,
        value: &Value,
    ) -> Result<serde_json::Value, QqlError> {
        match value {
            Value::Str(s) => Ok(serde_json::json!({
                "key": field,
                "match": { "value": s }
            })),
            Value::Int(i) => Ok(serde_json::json!({
                "key": field,
                "match": { "value": i }
            })),
            Value::Float(f) => Ok(exact_float_condition(field.to_string(), *f)),
            Value::Bool(b) => Ok(serde_json::json!({
                "key": field,
                "match": { "value": b }
            })),
            _ => Err(QqlError::runtime(
                "unsupported value type for equality match",
            )),
        }
    }

    fn build_not_equal_condition(
        &self,
        field: &str,
        value: &Value,
    ) -> Result<serde_json::Value, QqlError> {
        match value {
            Value::Str(s) => Ok(serde_json::json!({
                "key": field,
                "match": { "except": s }
            })),
            Value::Int(i) => Ok(serde_json::json!({
                "must_not": [
                    {
                        "key": field,
                        "match": { "value": i }
                    }
                ]
            })),
            Value::Float(f) => Ok(serde_json::json!({
                "must_not": [
                    exact_float_condition(field.to_string(), *f)
                ]
            })),
            Value::Bool(b) => Ok(serde_json::json!({
                "must_not": [
                    {
                        "key": field,
                        "match": { "value": b }
                    }
                ]
            })),
            _ => Err(QqlError::runtime(
                "unsupported value type for inequality match",
            )),
        }
    }

    fn build_set_condition(
        &self,
        field: &str,
        values: &[Value],
        negate: bool,
    ) -> Result<serde_json::Value, QqlError> {
        if values.is_empty() {
            let match_key = if negate { "except" } else { "any" };
            return Ok(serde_json::json!({
                "key": field,
                "match": { match_key: [] }
            }));
        }

        let kind = literal_kind_of(&values[0])?;
        for v in &values[1..] {
            let next_kind = literal_kind_of(v)?;
            if next_kind != kind {
                return Err(QqlError::runtime(
                    "mixed literal types are not supported in IN/NOT IN",
                ));
            }
        }

        match kind {
            LiteralKind::String => {
                let str_values: Vec<String> = values
                    .iter()
                    .map(|v| match v {
                        Value::Str(s) => s.to_string(),
                        _ => unreachable!(),
                    })
                    .collect();
                let match_key = if negate { "except" } else { "any" };
                Ok(serde_json::json!({
                    "key": field,
                    "match": { match_key: str_values }
                }))
            }
            LiteralKind::Int | LiteralKind::Float | LiteralKind::Bool => {
                let conds = build_scalar_conditions(field, values, &kind);
                if negate {
                    Ok(serde_json::json!({ "must_not": conds }))
                } else {
                    Ok(combine_conditions(conds))
                }
            }
        }
    }

    fn wrap_as_filter(&self, condition: serde_json::Value) -> serde_json::Value {
        if condition.get("must").is_some()
            || condition.get("must_not").is_some()
            || condition.get("should").is_some()
        {
            condition
        } else {
            serde_json::json!({
                "must": [condition]
            })
        }
    }
}

fn exact_float_condition(key: String, value: f64) -> serde_json::Value {
    serde_json::json!({
        "key": key,
        "range": {
            "gte": value,
            "lte": value
        }
    })
}

fn combine_conditions(conds: Vec<serde_json::Value>) -> serde_json::Value {
    if conds.len() == 1 {
        return conds.into_iter().next().unwrap();
    }
    serde_json::json!({
        "should": conds
    })
}

#[derive(PartialEq)]
enum LiteralKind {
    String,
    Int,
    Float,
    Bool,
}

fn literal_kind_of(value: &Value) -> Result<LiteralKind, QqlError> {
    match value {
        Value::Str(_) => Ok(LiteralKind::String),
        Value::Int(_) => Ok(LiteralKind::Int),
        Value::Float(_) => Ok(LiteralKind::Float),
        Value::Bool(_) => Ok(LiteralKind::Bool),
        _ => Err(QqlError::runtime("unsupported literal type")),
    }
}

/// Build scalar match conditions for Int, Float, and Bool values in an IN/NOT IN set.
fn build_scalar_conditions(
    field: &str,
    values: &[Value],
    kind: &LiteralKind,
) -> Vec<serde_json::Value> {
    values
        .iter()
        .map(|v| match (kind, v) {
            (LiteralKind::Int, Value::Int(i)) => serde_json::json!({
                "key": field,
                "match": { "value": i }
            }),
            (LiteralKind::Float, Value::Float(f)) => exact_float_condition(field.to_string(), *f),
            (LiteralKind::Bool, Value::Bool(b)) => serde_json::json!({
                "key": field,
                "match": { "value": b }
            }),
            _ => unreachable!(),
        })
        .collect()
}

fn to_float64(value: &Value) -> Result<Option<f64>, QqlError> {
    match value {
        Value::Float(f) => Ok(Some(*f)),
        Value::Int(i) => {
            let val = *i;
            if val.abs() > (1i64 << 53) {
                return Err(QqlError::runtime(
                    "integer too large: precision loss beyond 2^53 is not supported for range comparisons",
                ));
            }
            Ok(Some(val as f64))
        }
        _ => Err(QqlError::runtime(
            "expected numeric type for range condition",
        )),
    }
}
