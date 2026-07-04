use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{FilterExpr, Value};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, token_kind_to_op, Parser};

impl<'a> Parser<'a> {
    pub fn parse_filter_expr(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        let left = self.parse_filter_and()?;
        if self.peek()?.kind != TokenKind::Or {
            return Ok(left);
        }
        let mut operands = Vec::new();
        operands.push(left);
        while self.peek()?.kind == TokenKind::Or {
            self.advance()?;
            let right = self.parse_filter_and()?;
            operands.push(right);
        }
        Ok(FilterExpr::Or { operands })
    }

    pub fn parse_filter_and(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        let left = self.parse_filter_not()?;
        if self.peek()?.kind != TokenKind::And {
            return Ok(left);
        }
        let mut operands = Vec::new();
        operands.push(left);
        while self.peek()?.kind == TokenKind::And {
            self.advance()?;
            let right = self.parse_filter_not()?;
            operands.push(right);
        }
        Ok(FilterExpr::And { operands })
    }

    pub fn parse_filter_not(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        if self.peek()?.kind == TokenKind::Not {
            self.advance()?;
            let operand = self.parse_filter_not()?;
            return Ok(FilterExpr::Not {
                operand: Box::new(operand),
            });
        }
        self.parse_filter_primary()
    }

    pub fn parse_filter_primary(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        if self.peek()?.kind == TokenKind::Lparen {
            self.advance()?;
            let expr = self.parse_filter_expr()?;
            self.expect(TokenKind::Rparen)?;
            return Ok(expr);
        }
        if self.peek()?.kind == TokenKind::Identifier && ascii_equal(self.peek()?.text, "NESTED") {
            return self.parse_nested_function();
        }
        if self.peek()?.kind == TokenKind::HasVector {
            self.advance()?;
            let name_tok = self.expect(TokenKind::String)?;
            return Ok(FilterExpr::HasVector {
                name: name_tok.text,
            });
        }
        self.parse_predicate()
    }

    pub fn parse_nested_function(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Lparen)?;
        let path_tok = self.expect(TokenKind::String)?;
        self.expect(TokenKind::Comma)?;
        let inner = self.parse_filter_expr()?;
        self.expect(TokenKind::Rparen)?;
        Ok(FilterExpr::Nested {
            path: path_tok.text,
            filter: Box::new(inner),
        })
    }

    pub fn parse_predicate(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        let field = self.parse_field_path()?;
        let tok = self.peek()?;

        if tok.kind == TokenKind::Is {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Not {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Null {
                    self.advance()?;
                    return Ok(FilterExpr::IsNotNull { field });
                }
                if self.peek()?.kind == TokenKind::Empty {
                    self.advance()?;
                    return Ok(FilterExpr::IsNotEmpty { field });
                }
                return Err(QqlError::syntax(
                    "expected NULL or EMPTY after IS NOT",
                    self.peek()?.pos,
                ));
            }
            if self.peek()?.kind == TokenKind::Null {
                self.advance()?;
                return Ok(FilterExpr::IsNull { field });
            }
            if self.peek()?.kind == TokenKind::Empty {
                self.advance()?;
                return Ok(FilterExpr::IsEmpty { field });
            }
            return Err(QqlError::syntax(
                "expected NULL, NOT NULL, EMPTY, or NOT EMPTY after IS",
                self.peek()?.pos,
            ));
        }

        if tok.kind == TokenKind::In {
            self.advance()?;
            let values = self.parse_literal_list()?;
            return Ok(FilterExpr::In { field, values });
        }

        if tok.kind == TokenKind::Not {
            self.advance()?;
            self.expect(TokenKind::In)?;
            let values = self.parse_literal_list()?;
            return Ok(FilterExpr::NotIn { field, values });
        }

        if tok.kind == TokenKind::Between {
            self.advance()?;
            let low = self.parse_value()?;
            self.expect(TokenKind::And)?;
            let high = self.parse_value()?;
            return Ok(FilterExpr::Between { field, low, high });
        }

        if tok.kind == TokenKind::GeoBbox {
            let pos = tok.pos;
            self.advance()?;
            let val = self.parse_value()?;
            if let Value::Dict(ref items) = val {
                let mut top_left_lat = 0.0;
                let mut top_left_lon = 0.0;
                let mut bottom_right_lat = 0.0;
                let mut bottom_right_lon = 0.0;

                let top_left = items
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("top_left"));
                let bottom_right = items
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("bottom_right"));

                if let Some((_, Value::Dict(ref tl_items))) = top_left {
                    if let Some((_, ref v)) =
                        tl_items.iter().find(|(k, _)| k.eq_ignore_ascii_case("lat"))
                    {
                        top_left_lat = get_f64(v).unwrap_or(0.0);
                    }
                    if let Some((_, ref v)) =
                        tl_items.iter().find(|(k, _)| k.eq_ignore_ascii_case("lon"))
                    {
                        top_left_lon = get_f64(v).unwrap_or(0.0);
                    }
                }
                if let Some((_, Value::Dict(ref br_items))) = bottom_right {
                    if let Some((_, ref v)) =
                        br_items.iter().find(|(k, _)| k.eq_ignore_ascii_case("lat"))
                    {
                        bottom_right_lat = get_f64(v).unwrap_or(0.0);
                    }
                    if let Some((_, ref v)) =
                        br_items.iter().find(|(k, _)| k.eq_ignore_ascii_case("lon"))
                    {
                        bottom_right_lon = get_f64(v).unwrap_or(0.0);
                    }
                }

                return Ok(FilterExpr::GeoBoundingBox {
                    field,
                    top_left_lat,
                    top_left_lon,
                    bottom_right_lat,
                    bottom_right_lon,
                });
            }
            return Err(QqlError::syntax("GEO_BBOX requires a bounding box dictionary {top_left: {lat, lon}, bottom_right: {lat, lon}}", pos));
        }

        if tok.kind == TokenKind::GeoRadius {
            let pos = tok.pos;
            self.advance()?;
            let val = self.parse_value()?;
            if let Value::Dict(ref items) = val {
                let mut lat = 0.0;
                let mut lon = 0.0;
                let mut radius = 0.0;

                let center = items.iter().find(|(k, _)| k.eq_ignore_ascii_case("center"));
                let rad_val = items.iter().find(|(k, _)| k.eq_ignore_ascii_case("radius"));

                if let Some((_, Value::Dict(ref c_items))) = center {
                    if let Some((_, ref v)) =
                        c_items.iter().find(|(k, _)| k.eq_ignore_ascii_case("lat"))
                    {
                        lat = get_f64(v).unwrap_or(0.0);
                    }
                    if let Some((_, ref v)) =
                        c_items.iter().find(|(k, _)| k.eq_ignore_ascii_case("lon"))
                    {
                        lon = get_f64(v).unwrap_or(0.0);
                    }
                }
                if let Some((_, ref v)) = rad_val {
                    radius = get_f64(v).unwrap_or(0.0);
                }

                return Ok(FilterExpr::GeoRadius {
                    field,
                    lat,
                    lon,
                    radius,
                });
            }
            return Err(QqlError::syntax(
                "GEO_RADIUS requires a radius dictionary {center: {lat, lon}, radius: number}",
                pos,
            ));
        }

        if tok.kind == TokenKind::ValuesCount {
            self.advance()?;
            let op_tok = self.advance()?;
            let op = match op_tok.kind {
                TokenKind::Equals => "=",
                TokenKind::NotEquals => "!=",
                TokenKind::Gt => ">",
                TokenKind::Gte => ">=",
                TokenKind::Lt => "<",
                TokenKind::Lte => "<=",
                _ => {
                    return Err(QqlError::syntax(
                        "expected comparison operator after VALUES_COUNT",
                        op_tok.pos,
                    ))
                }
            };

            let count_tok = self.expect(TokenKind::Integer)?;
            let count = count_tok
                .text
                .parse::<i64>()
                .map_err(|_| QqlError::syntax("invalid integer count", count_tok.pos))?;

            return Ok(FilterExpr::ValuesCount { field, op, count });
        }

        if tok.kind == TokenKind::Match {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Any {
                self.advance()?;
                let text_tok = self.expect(TokenKind::String)?;
                return Ok(FilterExpr::MatchAny {
                    field,
                    text: text_tok.text,
                });
            }
            if self.peek()?.kind == TokenKind::Phrase {
                self.advance()?;
                let text_tok = self.expect(TokenKind::String)?;
                return Ok(FilterExpr::MatchPhrase {
                    field,
                    text: text_tok.text,
                });
            }
            let text_tok = self.expect(TokenKind::String)?;
            return Ok(FilterExpr::MatchText {
                field,
                text: text_tok.text,
            });
        }

        let op = token_kind_to_op(tok.kind);
        if !op.is_empty() {
            self.advance()?;
            let value = self.parse_value()?;
            return Ok(FilterExpr::Compare { field, op, value });
        }

        Err(QqlError::syntax(
            alloc::format!(
                "expected a filter operator after field '{}', got '{}'",
                field,
                tok.text
            ),
            tok.pos,
        ))
    }
}

fn get_f64(val: &crate::ast::Value) -> Option<f64> {
    match val {
        crate::ast::Value::Float(f) => Some(*f),
        crate::ast::Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}
