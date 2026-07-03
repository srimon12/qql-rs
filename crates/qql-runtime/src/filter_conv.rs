use qql_core::ast::{FilterExpr, Value};
use qql_core::error::QqlError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QdrantFilter {
    pub must: Option<Vec<QdrantCondition>>,
    pub must_not: Option<Vec<QdrantCondition>>,
    pub should: Option<Vec<QdrantCondition>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QdrantCondition {
    Match {
        key: String,
        value: FilterValue,
    },
    MatchExcept {
        key: String,
        value: FilterValue,
    },
    MatchKeywords {
        key: String,
        values: Vec<FilterValue>,
    },
    MatchExceptKeywords {
        key: String,
        values: Vec<FilterValue>,
    },
    Range {
        key: String,
        gt: Option<f64>,
        gte: Option<f64>,
        lt: Option<f64>,
        lte: Option<f64>,
    },
    IsNull {
        key: String,
    },
    IsEmpty {
        key: String,
    },
    Nested {
        key: String,
        filter: Box<QdrantFilter>,
    },
    HasId(Vec<FilterValue>),
    Boolean(Box<QdrantFilter>),
    MatchText {
        key: String,
        text: String,
    },
    MatchAny {
        key: String,
        text: String,
    },
    MatchPhrase {
        key: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl FilterValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FilterValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            FilterValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            FilterValue::Float(f) => Some(*f),
            FilterValue::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            FilterValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

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

    pub fn build_filter(&self, expr: &FilterExpr) -> Result<Option<QdrantFilter>, QqlError> {
        let condition = self.build_condition(expr)?;
        Ok(self.wrap_as_filter(condition))
    }

    fn build_condition(&self, expr: &FilterExpr) -> Result<QdrantCondition, QqlError> {
        match expr {
            FilterExpr::Compare { field, op, value } => self.build_compare_expr(field, op, value),
            FilterExpr::Between { field, low, high } => self.build_between_expr(field, low, high),
            FilterExpr::In { field, values } => self.build_in_expr(field, values),
            FilterExpr::NotIn { field, values } => self.build_not_in_expr(field, values),
            FilterExpr::IsNull { field } => Ok(QdrantCondition::IsNull {
                key: field.to_string(),
            }),
            FilterExpr::IsNotNull { field } => {
                Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                    must_not: Some(vec![QdrantCondition::IsNull {
                        key: field.to_string(),
                    }]),
                    must: None,
                    should: None,
                })))
            }
            FilterExpr::IsEmpty { field } => Ok(QdrantCondition::IsEmpty {
                key: field.to_string(),
            }),
            FilterExpr::IsNotEmpty { field } => {
                Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                    must_not: Some(vec![QdrantCondition::IsEmpty {
                        key: field.to_string(),
                    }]),
                    must: None,
                    should: None,
                })))
            }
            FilterExpr::MatchText { field, text } => Ok(QdrantCondition::MatchText {
                key: field.to_string(),
                text: text.to_string(),
            }),
            FilterExpr::MatchAny { field, text } => Ok(QdrantCondition::MatchAny {
                key: field.to_string(),
                text: text.to_string(),
            }),
            FilterExpr::MatchPhrase { field, text } => Ok(QdrantCondition::MatchPhrase {
                key: field.to_string(),
                text: text.to_string(),
            }),
            FilterExpr::And { operands } => self.build_and_expr(operands),
            FilterExpr::Or { operands } => self.build_or_expr(operands),
            FilterExpr::Not { operand } => self.build_not_expr(operand),
            FilterExpr::Nested { path, filter } => self.build_nested_expr(path, filter),
        }
    }

    fn build_compare_expr(
        &self,
        field: &str,
        op: &str,
        value: &Value,
    ) -> Result<QdrantCondition, QqlError> {
        match op {
            "=" => self.build_equal_condition(field, value),
            "!=" => self.build_not_equal_condition(field, value),
            ">" => {
                let v = to_float64(value)?;
                Ok(QdrantCondition::Range {
                    key: field.to_string(),
                    gt: v,
                    gte: None,
                    lt: None,
                    lte: None,
                })
            }
            ">=" => {
                let v = to_float64(value)?;
                Ok(QdrantCondition::Range {
                    key: field.to_string(),
                    gt: None,
                    gte: v,
                    lt: None,
                    lte: None,
                })
            }
            "<" => {
                let v = to_float64(value)?;
                Ok(QdrantCondition::Range {
                    key: field.to_string(),
                    gt: None,
                    gte: None,
                    lt: v,
                    lte: None,
                })
            }
            "<=" => {
                let v = to_float64(value)?;
                Ok(QdrantCondition::Range {
                    key: field.to_string(),
                    gt: None,
                    gte: None,
                    lt: None,
                    lte: v,
                })
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
    ) -> Result<QdrantCondition, QqlError> {
        let low_v = to_float64(low)?;
        let high_v = to_float64(high)?;
        Ok(QdrantCondition::Range {
            key: field.to_string(),
            gt: None,
            gte: low_v,
            lt: None,
            lte: high_v,
        })
    }

    fn build_in_expr(&self, field: &str, values: &[Value]) -> Result<QdrantCondition, QqlError> {
        self.build_set_condition(field, values, false)
    }

    fn build_not_in_expr(
        &self,
        field: &str,
        values: &[Value],
    ) -> Result<QdrantCondition, QqlError> {
        self.build_set_condition(field, values, true)
    }

    fn build_and_expr(&self, operands: &[FilterExpr]) -> Result<QdrantCondition, QqlError> {
        let mut must = Vec::with_capacity(operands.len());
        for operand in operands {
            let cond = self.build_condition(operand)?;
            must.push(cond);
        }
        Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
            must: Some(must),
            must_not: None,
            should: None,
        })))
    }

    fn build_or_expr(&self, operands: &[FilterExpr]) -> Result<QdrantCondition, QqlError> {
        let mut should = Vec::with_capacity(operands.len());
        for operand in operands {
            let cond = self.build_condition(operand)?;
            should.push(cond);
        }
        Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
            must: None,
            must_not: Some(Vec::new()),
            should: Some(should),
        })))
    }

    fn build_not_expr(&self, operand: &FilterExpr) -> Result<QdrantCondition, QqlError> {
        let cond = self.build_condition(operand)?;
        Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
            must: None,
            must_not: Some(vec![cond]),
            should: None,
        })))
    }

    fn build_nested_expr(
        &self,
        path: &str,
        filter: &FilterExpr,
    ) -> Result<QdrantCondition, QqlError> {
        let inner = self.build_filter(filter)?;
        match inner {
            Some(f) => Ok(QdrantCondition::Nested {
                key: path.to_string(),
                filter: Box::new(f),
            }),
            None => Err(QqlError::runtime("empty nested filter")),
        }
    }

    fn build_equal_condition(
        &self,
        field: &str,
        value: &Value,
    ) -> Result<QdrantCondition, QqlError> {
        match value {
            Value::Str(s) => Ok(QdrantCondition::Match {
                key: field.to_string(),
                value: FilterValue::Str(s.to_string()),
            }),
            Value::Int(i) => Ok(QdrantCondition::MatchKeywords {
                key: field.to_string(),
                values: vec![FilterValue::Int(*i)],
            }),
            Value::Float(f) => Ok(exact_float_condition(field.to_string(), *f)),
            Value::Bool(b) => Ok(QdrantCondition::Match {
                key: field.to_string(),
                value: FilterValue::Bool(*b),
            }),
            _ => Err(QqlError::runtime(
                "unsupported value type for equality match",
            )),
        }
    }

    fn build_not_equal_condition(
        &self,
        field: &str,
        value: &Value,
    ) -> Result<QdrantCondition, QqlError> {
        match value {
            Value::Str(s) => Ok(QdrantCondition::MatchExcept {
                key: field.to_string(),
                value: FilterValue::Str(s.to_string()),
            }),
            Value::Int(i) => Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                must_not: Some(vec![QdrantCondition::MatchKeywords {
                    key: field.to_string(),
                    values: vec![FilterValue::Int(*i)],
                }]),
                must: None,
                should: None,
            }))),
            Value::Float(f) => Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                must_not: Some(vec![exact_float_condition(field.to_string(), *f)]),
                must: None,
                should: None,
            }))),
            Value::Bool(b) => Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                must_not: Some(vec![QdrantCondition::Match {
                    key: field.to_string(),
                    value: FilterValue::Bool(*b),
                }]),
                must: None,
                should: None,
            }))),
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
    ) -> Result<QdrantCondition, QqlError> {
        if values.is_empty() {
            if negate {
                return Ok(QdrantCondition::MatchExceptKeywords {
                    key: field.to_string(),
                    values: Vec::new(),
                });
            }
            return Ok(QdrantCondition::MatchKeywords {
                key: field.to_string(),
                values: Vec::new(),
            });
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
                let str_values: Vec<FilterValue> = values
                    .iter()
                    .map(|v| match v {
                        Value::Str(s) => FilterValue::Str(s.to_string()),
                        _ => unreachable!(),
                    })
                    .collect();
                if negate {
                    Ok(QdrantCondition::MatchExceptKeywords {
                        key: field.to_string(),
                        values: str_values,
                    })
                } else {
                    Ok(QdrantCondition::MatchKeywords {
                        key: field.to_string(),
                        values: str_values,
                    })
                }
            }
            LiteralKind::Int => {
                let _int_values: Vec<FilterValue> = values
                    .iter()
                    .map(|v| match v {
                        Value::Int(i) => FilterValue::Int(*i),
                        _ => unreachable!(),
                    })
                    .collect();
                if negate {
                    Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                        must_not: Some(
                            values
                                .iter()
                                .map(|v| match v {
                                    Value::Int(i) => QdrantCondition::MatchKeywords {
                                        key: field.to_string(),
                                        values: vec![FilterValue::Int(*i)],
                                    },
                                    _ => unreachable!(),
                                })
                                .collect(),
                        ),
                        must: None,
                        should: None,
                    })))
                } else {
                    Ok(combine_conditions(
                        values
                            .iter()
                            .map(|v| match v {
                                Value::Int(i) => QdrantCondition::MatchKeywords {
                                    key: field.to_string(),
                                    values: vec![FilterValue::Int(*i)],
                                },
                                _ => unreachable!(),
                            })
                            .collect(),
                    ))
                }
            }
            LiteralKind::Float => {
                let _float_values: Vec<FilterValue> = values
                    .iter()
                    .map(|v| match v {
                        Value::Float(f) => FilterValue::Float(*f),
                        _ => unreachable!(),
                    })
                    .collect();
                let conds: Vec<QdrantCondition> = values
                    .iter()
                    .map(|v| match v {
                        Value::Float(f) => exact_float_condition(field.to_string(), *f),
                        _ => unreachable!(),
                    })
                    .collect();
                if negate {
                    Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                        must_not: Some(conds),
                        must: None,
                        should: None,
                    })))
                } else {
                    Ok(combine_conditions(conds))
                }
            }
            LiteralKind::Bool => {
                let conds: Vec<QdrantCondition> = values
                    .iter()
                    .map(|v| match v {
                        Value::Bool(b) => QdrantCondition::Match {
                            key: field.to_string(),
                            value: FilterValue::Bool(*b),
                        },
                        _ => unreachable!(),
                    })
                    .collect();
                if negate {
                    Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
                        must_not: Some(conds),
                        must: None,
                        should: None,
                    })))
                } else {
                    Ok(combine_conditions(conds))
                }
            }
        }
    }

    fn wrap_as_filter(&self, condition: QdrantCondition) -> Option<QdrantFilter> {
        match &condition {
            QdrantCondition::Boolean(filter) => Some(filter.as_ref().clone()),
            _ => Some(QdrantFilter {
                must: Some(vec![condition]),
                must_not: None,
                should: None,
            }),
        }
    }
}

fn exact_float_condition(key: String, value: f64) -> QdrantCondition {
    QdrantCondition::Range {
        key,
        gt: None,
        gte: Some(value),
        lt: None,
        lte: Some(value),
    }
}

fn combine_conditions(conds: Vec<QdrantCondition>) -> QdrantCondition {
    if conds.len() == 1 {
        return conds.into_iter().next().unwrap();
    }
    QdrantCondition::Boolean(Box::new(QdrantFilter {
        must: None,
        must_not: None,
        should: Some(conds),
    }))
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

fn to_float64(value: &Value) -> Result<Option<f64>, QqlError> {
    match value {
        Value::Float(f) => Ok(Some(*f)),
        Value::Int(i) => Ok(Some(*i as f64)),
        _ => Err(QqlError::runtime(
            "expected numeric type for range condition",
        )),
    }
}
