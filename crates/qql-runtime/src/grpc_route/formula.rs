//! Formula expression converters: plan AST / REST-shaped JSON → proto.

use crate::qdrant_grpc::qdrant;

pub(crate) fn ast_formula_to_grpc(expr: &qql_core::ast::FormulaExpr) -> Option<qdrant::Expression> {
    use qdrant::expression::Variant;
    match expr {
        qql_core::ast::FormulaExpr::Constant { value } => Some(qdrant::Expression {
            variant: Some(Variant::Constant(*value as f32)),
        }),
        qql_core::ast::FormulaExpr::Variable { name } => Some(qdrant::Expression {
            variant: Some(Variant::Variable(if name == "score" {
                "$score".to_string()
            } else {
                name.clone()
            })),
        }),
        qql_core::ast::FormulaExpr::Sum { left, right } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Sum(qdrant::SumExpression { sum: vec![l, r] })),
            })
        }
        qql_core::ast::FormulaExpr::Sub { left, right } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            let neg_r = qdrant::Expression {
                variant: Some(Variant::Neg(Box::new(r))),
            };
            Some(qdrant::Expression {
                variant: Some(Variant::Sum(qdrant::SumExpression {
                    sum: vec![l, neg_r],
                })),
            })
        }
        qql_core::ast::FormulaExpr::Mul { left, right } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Mult(qdrant::MultExpression { mult: vec![l, r] })),
            })
        }
        qql_core::ast::FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Div(Box::new(qdrant::DivExpression {
                    left: Some(Box::new(l)),
                    right: Some(Box::new(r)),
                    by_zero_default: by_zero_default.map(|f| f as f32),
                }))),
            })
        }
        qql_core::ast::FormulaExpr::Neg { operand } => {
            let inner = ast_formula_to_grpc(operand)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Neg(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Abs { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Abs(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Sqrt { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Sqrt(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Log { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Log10(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Ln { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Ln(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Exp { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Exp(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Pow { base, exponent } => {
            let b = ast_formula_to_grpc(base)?;
            let e = ast_formula_to_grpc(exponent)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                    base: Some(Box::new(b)),
                    exponent: Some(Box::new(e)),
                }))),
            })
        }
        qql_core::ast::FormulaExpr::GeoDistance { lat, lon, field } => Some(qdrant::Expression {
            variant: Some(Variant::GeoDistance(qdrant::GeoDistance {
                origin: Some(qdrant::GeoPoint {
                    lat: *lat,
                    lon: *lon,
                }),
                to: field.clone(),
            })),
        }),
        qql_core::ast::FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let x_expr = ast_formula_to_grpc(x)?;
            let target_expr = match target {
                Some(t) => Some(Box::new(ast_formula_to_grpc(t)?)),
                None => None,
            };
            let decay = Box::new(qdrant::DecayParamsExpression {
                x: Some(Box::new(x_expr)),
                target: target_expr,
                scale: scale.map(|f| f as f32),
                midpoint: midpoint.map(|f| f as f32),
            });
            let variant = match kind.to_ascii_lowercase().as_str() {
                "exp" | "exp_decay" => Variant::ExpDecay(decay),
                "lin" | "lin_decay" => Variant::LinDecay(decay),
                _ => Variant::GaussDecay(decay),
            };
            Some(qdrant::Expression {
                variant: Some(variant),
            })
        }
        _ => to_formula_expression(&qql_plan::query::lower_formula_expr(expr)),
    }
}

pub(crate) fn to_formula_expression(val: &serde_json::Value) -> Option<qdrant::Expression> {
    use qdrant::expression::Variant;
    // OpenAPI Expression: bare number / bare string / one-key objects with snake_case keys.
    match val {
        serde_json::Value::Number(n) => n.as_f64().map(|f| qdrant::Expression {
            variant: Some(Variant::Constant(f as f32)),
        }),
        serde_json::Value::String(s) => Some(qdrant::Expression {
            variant: Some(Variant::Variable(s.clone())),
        }),
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, val) = obj.iter().next()?;
            match key.as_str() {
                // REST dialect (qql-plan output) + legacy PascalCase keys
                "Constant" | "constant" => val.as_f64().map(|f| qdrant::Expression {
                    variant: Some(Variant::Constant(f as f32)),
                }),
                "Variable" | "variable" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Variable(s.to_string())),
                }),
                "sum" | "Add" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression { sum: terms })),
                    })
                }
                "mult" | "Multiply" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression { mult: terms })),
                    })
                }
                "div" | "Divide" => {
                    let obj = val.as_object()?;
                    let left = to_formula_expression(obj.get("left")?)?;
                    let right = to_formula_expression(obj.get("right")?)?;
                    let by_zero_default = obj
                        .get("by_zero_default")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    Some(qdrant::Expression {
                        variant: Some(Variant::Div(Box::new(qdrant::DivExpression {
                            left: Some(Box::new(left)),
                            right: Some(Box::new(right)),
                            by_zero_default,
                        }))),
                    })
                }
                "neg" | "Negate" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Neg(Box::new(inner))),
                    })
                }
                "abs" | "Abs" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Abs(Box::new(inner))),
                    })
                }
                "sqrt" | "Sqrt" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sqrt(Box::new(inner))),
                    })
                }
                "log10" | "Log10" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Log10(Box::new(inner))),
                    })
                }
                "ln" | "NaturalLog" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Ln(Box::new(inner))),
                    })
                }
                "exp" | "Exp" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Exp(Box::new(inner))),
                    })
                }
                "pow" | "Pow" => {
                    if let Some(arr) = val.as_array() {
                        Some(qdrant::Expression {
                            variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                                base: Some(Box::new(to_formula_expression(&arr[0])?)),
                                exponent: Some(Box::new(to_formula_expression(&arr[1])?)),
                            }))),
                        })
                    } else {
                        let obj = val.as_object()?;
                        Some(qdrant::Expression {
                            variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                                base: Some(Box::new(to_formula_expression(obj.get("base")?)?)),
                                exponent: Some(Box::new(to_formula_expression(
                                    obj.get("exponent")?,
                                )?)),
                            }))),
                        })
                    }
                }
                "geo_distance" | "GeoDistance" => {
                    let obj = val.as_object()?;
                    let origin = obj.get("origin")?;
                    let lat = origin
                        .get("lat")
                        .or_else(|| origin.get("latitude"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let lon = origin
                        .get("lon")
                        .or_else(|| origin.get("longitude"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let to = obj
                        .get("to")
                        .or_else(|| obj.get("field"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(qdrant::Expression {
                        variant: Some(Variant::GeoDistance(qdrant::GeoDistance {
                            origin: Some(qdrant::GeoPoint { lat, lon }),
                            to,
                        })),
                    })
                }
                "exp_decay" | "gauss_decay" | "lin_decay" => {
                    let obj = val.as_object()?;
                    let x = to_formula_expression(obj.get("x")?)?;
                    let target = obj.get("target").and_then(to_formula_expression);
                    let scale = obj.get("scale").and_then(|v| v.as_f64()).map(|f| f as f32);
                    let midpoint = obj
                        .get("midpoint")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    let decay = Box::new(qdrant::DecayParamsExpression {
                        x: Some(Box::new(x)),
                        target: target.map(Box::new),
                        scale,
                        midpoint,
                    });
                    let variant = match key.as_str() {
                        "exp_decay" => Variant::ExpDecay(decay),
                        "lin_decay" => Variant::LinDecay(decay),
                        _ => Variant::GaussDecay(decay),
                    };
                    Some(qdrant::Expression {
                        variant: Some(variant),
                    })
                }
                "datetime" | "DateTime" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Datetime(s.to_string())),
                }),
                "datetime_key" | "DateTimeField" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::DatetimeKey(s.to_string())),
                }),
                // Field condition used as boolean Expression (key + match/range/...)
                "key" => {
                    // Reconstruct single-key object for condition parser
                    to_condition_from_json(&serde_json::Value::Object(obj.clone())).map(|c| {
                        qdrant::Expression {
                            variant: Some(Variant::Condition(c)),
                        }
                    })
                }
                _ => {
                    // Try as a filter condition object (must/should/must_not or field clause).
                    to_condition_from_json(val).map(|c| qdrant::Expression {
                        variant: Some(Variant::Condition(c)),
                    })
                }
            }
        }
        // Multi-key objects: likely a field condition {key, match}
        serde_json::Value::Object(obj) => {
            to_condition_from_json(&serde_json::Value::Object(obj.clone())).map(|c| {
                qdrant::Expression {
                    variant: Some(Variant::Condition(c)),
                }
            })
        }
        _ => None,
    }
}

pub(crate) fn to_condition_from_json(val: &serde_json::Value) -> Option<qdrant::Condition> {
    match val {
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, inner) = obj.iter().next()?;
            match key.as_str() {
                "And" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Or" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                should: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Not" => {
                    let inner_cond = to_condition_from_json(inner)?;
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must_not: vec![inner_cond],
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Compare" => {
                    let obj = inner.as_object()?;
                    let field = obj.get("field")?.as_str()?;
                    let op = obj.get("op")?.as_str()?;
                    let value = obj.get("value")?;
                    let range = match op {
                        "Eq" => qdrant::Range {
                            gte: value.as_f64(),
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        "Gt" => qdrant::Range {
                            gt: value.as_f64(),
                            ..Default::default()
                        },
                        "Gte" => qdrant::Range {
                            gte: value.as_f64(),
                            ..Default::default()
                        },
                        "Lt" => qdrant::Range {
                            lt: value.as_f64(),
                            ..Default::default()
                        },
                        "Lte" => qdrant::Range {
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        _ => return None,
                    };
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Field(
                            qdrant::FieldCondition {
                                key: field.to_string(),
                                range: Some(range),
                                ..Default::default()
                            },
                        )),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}
