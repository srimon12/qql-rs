use super::helpers::point_id_from_value;
use super::{ascii_equal, Parser};
use crate::ast::{ComparisonOp, FilterExpr, GeoPoint, PointIdPredicate, Value};
use crate::error::{QqlError, Span};
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::vec::Vec;

impl<'a> Parser<'a> {
    pub fn parse_filter_expr(&mut self) -> Result<FilterExpr, QqlError> {
        let first = self.parse_filter_and()?;
        if self.peek()?.kind != TokenKind::Or {
            return Ok(first);
        }
        let mut operands = alloc::vec![first];
        while self.peek()?.kind == TokenKind::Or {
            self.advance()?;
            operands.push(self.parse_filter_and()?);
        }
        Ok(FilterExpr::Or { operands })
    }

    fn parse_filter_and(&mut self) -> Result<FilterExpr, QqlError> {
        let first = self.parse_filter_not()?;
        if self.peek()?.kind != TokenKind::And {
            return Ok(first);
        }
        let mut operands = alloc::vec![first];
        while self.peek()?.kind == TokenKind::And {
            self.advance()?;
            operands.push(self.parse_filter_not()?);
        }
        Ok(FilterExpr::And { operands })
    }

    fn parse_filter_not(&mut self) -> Result<FilterExpr, QqlError> {
        if self.peek()?.kind == TokenKind::Not {
            self.advance()?;
            return Ok(FilterExpr::Not {
                operand: Box::new(self.parse_filter_not()?),
            });
        }
        self.parse_filter_primary()
    }

    fn parse_filter_primary(&mut self) -> Result<FilterExpr, QqlError> {
        if self.peek()?.kind == TokenKind::Lparen {
            self.advance()?;
            let expression = self.parse_filter_expr()?;
            self.expect(TokenKind::Rparen)?;
            return Ok(expression);
        }
        if self.peek()?.kind == TokenKind::Identifier && ascii_equal(self.peek()?.text, "NESTED") {
            self.advance()?;
            self.expect(TokenKind::Lparen)?;
            let path = self.parse_string()?;
            self.expect(TokenKind::Comma)?;
            let filter = self.parse_filter_expr()?;
            self.expect(TokenKind::Rparen)?;
            return Ok(FilterExpr::Nested {
                path,
                filter: Box::new(filter),
            });
        }
        if self.peek()?.kind == TokenKind::HasVector {
            self.advance()?;
            return Ok(FilterExpr::HasVector {
                name: self.parse_identifier()?,
            });
        }
        self.parse_predicate()
    }

    fn parse_predicate(&mut self) -> Result<FilterExpr, QqlError> {
        let field_token = self.peek()?;
        let field = self.parse_field_path()?;
        let point_id = field.eq_ignore_ascii_case("id");
        let operator = self.peek()?;

        if operator.kind == TokenKind::Is {
            if point_id {
                return Err(id_operator_error(operator.span));
            }
            self.advance()?;
            let negate = if self.peek()?.kind == TokenKind::Not {
                self.advance()?;
                true
            } else {
                false
            };
            let expression = match self.peek()?.kind {
                TokenKind::Null => {
                    self.advance()?;
                    FilterExpr::IsNull { field }
                }
                TokenKind::Empty => {
                    self.advance()?;
                    FilterExpr::IsEmpty { field }
                }
                _ => {
                    return Err(QqlError::parse(
                        "QQL-PARSE-FILTER",
                        "IS requires NULL, EMPTY, NOT NULL, or NOT EMPTY",
                        self.peek()?.span,
                    ));
                }
            };
            return Ok(if negate { not(expression) } else { expression });
        }

        if operator.kind == TokenKind::In {
            self.advance()?;
            let values = self.parse_literal_list()?;
            return if point_id {
                point_ids(values, field_token.span)
                    .map(|ids| FilterExpr::PointId(PointIdPredicate::In(ids)))
            } else {
                Ok(FilterExpr::In { field, values })
            };
        }

        if operator.kind == TokenKind::Not {
            self.advance()?;
            self.expect(TokenKind::In)?;
            let values = self.parse_literal_list()?;
            let expression = if point_id {
                FilterExpr::PointId(PointIdPredicate::In(point_ids(values, field_token.span)?))
            } else {
                FilterExpr::In { field, values }
            };
            return Ok(not(expression));
        }

        if operator.kind == TokenKind::Between {
            if point_id {
                return Err(id_operator_error(operator.span));
            }
            self.advance()?;
            let low = self.parse_value()?;
            self.expect(TokenKind::And)?;
            let high = self.parse_value()?;
            return Ok(FilterExpr::Between { field, low, high });
        }

        if operator.kind == TokenKind::GeoBbox {
            if point_id {
                return Err(id_operator_error(operator.span));
            }
            self.advance()?;
            let span = self.peek()?.span;
            let Value::Dict(items) = self.parse_value()? else {
                return Err(geo_error("GEO_BBOX requires an object", span));
            };
            let top_left = geo_point(dict_value(&items, "top_left"), "top_left", span)?;
            let bottom_right = geo_point(dict_value(&items, "bottom_right"), "bottom_right", span)?;
            return Ok(FilterExpr::GeoBoundingBox {
                field,
                top_left,
                bottom_right,
            });
        }

        if operator.kind == TokenKind::GeoRadius {
            if point_id {
                return Err(id_operator_error(operator.span));
            }
            self.advance()?;
            let span = self.peek()?.span;
            let Value::Dict(items) = self.parse_value()? else {
                return Err(geo_error("GEO_RADIUS requires an object", span));
            };
            let center = geo_point(dict_value(&items, "center"), "center", span)?;
            let radius = number(dict_value(&items, "radius"), "radius", span)?;
            if radius <= 0.0 {
                return Err(geo_error("radius must be greater than zero", span));
            }
            return Ok(FilterExpr::GeoRadius {
                field,
                center,
                radius,
            });
        }

        if operator.kind == TokenKind::ValuesCount {
            if point_id {
                return Err(id_operator_error(operator.span));
            }
            self.advance()?;
            let comparison = self.parse_comparison_operator()?;
            let count = self.parse_non_negative_u64("VALUES_COUNT")?;
            let expression = FilterExpr::ValuesCount {
                field,
                op: comparison.0,
                count,
            };
            return Ok(if comparison.1 {
                not(expression)
            } else {
                expression
            });
        }

        if operator.kind == TokenKind::Match {
            if point_id {
                return Err(id_operator_error(operator.span));
            }
            self.advance()?;
            if self.peek()?.kind == TokenKind::Any {
                self.advance()?;
                let values = self.parse_literal_list()?;
                if values.is_empty() {
                    return Err(QqlError::parse(
                        "QQL-PARSE-MATCH-ANY",
                        "MATCH ANY requires a non-empty exact-value list",
                        self.peek()?.span,
                    ));
                }
                return Ok(FilterExpr::MatchAny { field, values });
            }
            if self.peek()?.kind == TokenKind::Phrase {
                self.advance()?;
                return Ok(FilterExpr::MatchPhrase {
                    field,
                    text: self.parse_string()?,
                });
            }
            return Ok(FilterExpr::MatchText {
                field,
                text: self.parse_string()?,
            });
        }

        if matches!(
            operator.kind,
            TokenKind::Equals
                | TokenKind::NotEquals
                | TokenKind::Gt
                | TokenKind::Gte
                | TokenKind::Lt
                | TokenKind::Lte
        ) {
            let (comparison, negate) = self.parse_comparison_operator()?;
            let value_span = self.peek()?.span;
            let value = self.parse_value()?;
            let expression = if point_id {
                if comparison != ComparisonOp::Eq {
                    return Err(id_operator_error(operator.span));
                }
                FilterExpr::PointId(PointIdPredicate::Eq(point_id_from_value(
                    value, value_span,
                )?))
            } else {
                FilterExpr::Compare {
                    field,
                    op: comparison,
                    value,
                }
            };
            return Ok(if negate { not(expression) } else { expression });
        }

        Err(QqlError::parse(
            "QQL-PARSE-FILTER",
            alloc::format!("expected a filter operator after '{}'", field),
            operator.span,
        ))
    }

    fn parse_comparison_operator(&mut self) -> Result<(ComparisonOp, bool), QqlError> {
        let token = self.advance()?;
        match token.kind {
            TokenKind::Equals => Ok((ComparisonOp::Eq, false)),
            TokenKind::NotEquals => Ok((ComparisonOp::Eq, true)),
            TokenKind::Gt => Ok((ComparisonOp::Gt, false)),
            TokenKind::Gte => Ok((ComparisonOp::Gte, false)),
            TokenKind::Lt => Ok((ComparisonOp::Lt, false)),
            TokenKind::Lte => Ok((ComparisonOp::Lte, false)),
            _ => Err(QqlError::parse(
                "QQL-PARSE-COMPARISON",
                "expected a comparison operator",
                token.span,
            )),
        }
    }
}

fn not(expression: FilterExpr) -> FilterExpr {
    FilterExpr::Not {
        operand: Box::new(expression),
    }
}

fn point_ids(values: Vec<Value>, span: Span) -> Result<Vec<crate::ast::PointId>, QqlError> {
    values
        .into_iter()
        .map(|value| point_id_from_value(value, span))
        .collect()
}

fn dict_value<'a>(values: &'a [(alloc::string::String, Value)], key: &str) -> Option<&'a Value> {
    values
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

fn geo_point(value: Option<&Value>, name: &str, span: Span) -> Result<GeoPoint, QqlError> {
    let Some(Value::Dict(values)) = value else {
        return Err(geo_error(
            alloc::format!("{} must be an object with lat and lon", name),
            span,
        ));
    };
    let lat = number(dict_value(values, "lat"), "lat", span)?;
    let lon = number(dict_value(values, "lon"), "lon", span)?;
    if !(-90.0..=90.0).contains(&lat) {
        return Err(geo_error("latitude must be between -90 and 90", span));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(geo_error("longitude must be between -180 and 180", span));
    }
    Ok(GeoPoint { lat, lon })
}

fn number(value: Option<&Value>, name: &str, span: Span) -> Result<f64, QqlError> {
    let value = match value {
        Some(Value::Int(value)) => *value as f64,
        Some(Value::Float(value)) => *value,
        _ => {
            return Err(geo_error(alloc::format!("{} must be numeric", name), span));
        }
    };
    if !value.is_finite() {
        return Err(geo_error(alloc::format!("{} must be finite", name), span));
    }
    Ok(value)
}

fn id_operator_error(span: Span) -> QqlError {
    QqlError::validation(
        "QQL-VALIDATION-ID-PREDICATE",
        "point ID predicates support only =, !=, IN, and NOT IN",
        Some(span),
    )
}

fn geo_error(message: impl Into<alloc::borrow::Cow<'static, str>>, span: Span) -> QqlError {
    QqlError::validation("QQL-VALIDATION-GEO", message, Some(span))
}
