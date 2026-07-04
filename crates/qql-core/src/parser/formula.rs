use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::ast::{FormulaExpr, Value};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, Parser};

// ── Precedence constants ────────────────────────────────────────

pub const PRECEDENCE_LOWEST: u8 = 0;
const PRECEDENCE_SUM: u8 = 1;
const PRECEDENCE_PRODUCT: u8 = 2;
const PRECEDENCE_PREFIX: u8 = 3;

fn token_precedence(kind: TokenKind) -> u8 {
    match kind {
        TokenKind::Plus | TokenKind::Minus => PRECEDENCE_SUM,
        TokenKind::Star | TokenKind::Slash => PRECEDENCE_PRODUCT,
        _ => PRECEDENCE_LOWEST,
    }
}

type PrefixParseFn = for<'a> fn(&mut Parser<'a>) -> Result<FormulaExpr<'a>, QqlError>;
type InfixParseFn =
    for<'a> fn(&mut Parser<'a>, FormulaExpr<'a>) -> Result<FormulaExpr<'a>, QqlError>;

fn formula_prefix_parse_fn(kind: TokenKind) -> Option<PrefixParseFn> {
    match kind {
        TokenKind::Identifier
        | TokenKind::Score
        | TokenKind::Offset
        | TokenKind::Threshold
        | TokenKind::Lookup
        | TokenKind::Match => Some(parse_formula_identifier_or_func),
        TokenKind::Integer | TokenKind::Float => Some(parse_formula_constant),
        TokenKind::Minus => Some(parse_formula_prefix_expression),
        TokenKind::Lparen => Some(parse_formula_grouped_expression),
        TokenKind::Case => Some(parse_formula_case_expression),
        _ => None,
    }
}

fn formula_infix_parse_fn(kind: TokenKind) -> Option<InfixParseFn> {
    match kind {
        TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash => {
            Some(parse_formula_infix_expression)
        }
        _ => None,
    }
}

impl<'a> Parser<'a> {
    pub fn parse_formula_expr(&mut self, precedence: u8) -> Result<FormulaExpr<'a>, QqlError> {
        let tok = self.peek()?;
        let prefix = formula_prefix_parse_fn(tok.kind).ok_or_else(|| {
            QqlError::syntax(
                alloc::format!("unexpected token in formula: {}", tok.text),
                tok.pos,
            )
        })?;

        let mut left = prefix(self)?;

        while self.peek()?.kind != TokenKind::Eof
            && precedence < token_precedence(self.peek()?.kind)
        {
            let infix = formula_infix_parse_fn(self.peek()?.kind);
            match infix {
                None => return Ok(left),
                Some(infix) => {
                    left = infix(self, left)?;
                }
            }
        }

        Ok(left)
    }
}

// ── Prefix parse functions ──────────────────────────────────────

fn parse_formula_identifier_or_func<'a>(p: &mut Parser<'a>) -> Result<FormulaExpr<'a>, QqlError> {
    let tok = p.advance()?;
    let val = tok.text;
    let lower = val.to_ascii_lowercase();

    if p.peek()?.kind == TokenKind::Lparen {
        p.advance()?;
        return parse_formula_function_call(p, &lower, tok.pos);
    }

    Ok(FormulaExpr::Variable { name: val })
}

fn parse_formula_constant<'a>(p: &mut Parser<'a>) -> Result<FormulaExpr<'a>, QqlError> {
    let tok = p.advance()?;
    let v: f64 = tok
        .text
        .parse()
        .map_err(|_| QqlError::syntax("invalid number format in formula", tok.pos))?;
    Ok(FormulaExpr::Constant { value: v })
}

fn parse_formula_prefix_expression<'a>(p: &mut Parser<'a>) -> Result<FormulaExpr<'a>, QqlError> {
    p.advance()?;
    let right = p.parse_formula_expr(PRECEDENCE_PREFIX)?;
    Ok(FormulaExpr::Neg {
        operand: Box::new(right),
    })
}

// ── Infix parse functions ───────────────────────────────────────

fn parse_formula_infix_expression<'a>(
    p: &mut Parser<'a>,
    left: FormulaExpr<'a>,
) -> Result<FormulaExpr<'a>, QqlError> {
    let tok = p.advance()?;
    let prec = token_precedence(tok.kind);
    let right = p.parse_formula_expr(prec)?;

    match tok.kind {
        TokenKind::Plus => Ok(FormulaExpr::Sum {
            left: Box::new(left),
            right: Box::new(right),
        }),
        TokenKind::Minus => Ok(FormulaExpr::Sub {
            left: Box::new(left),
            right: Box::new(right),
        }),
        TokenKind::Star => Ok(FormulaExpr::Mul {
            left: Box::new(left),
            right: Box::new(right),
        }),
        TokenKind::Slash => {
            let mut by_zero_default = None;
            if p.peek()?.kind == TokenKind::Lbracket
                && p.index + 1 < p.tokens.len()
            {
                let next_tok = &p.tokens[p.index + 1];
                if next_tok.kind == TokenKind::Identifier
                    && ascii_equal(next_tok.text, "DEFAULT")
                {
                        p.advance()?;
                        p.advance()?;
                        p.expect(TokenKind::Equals)?;
                        let val = p.parse_numeric_literal()?;
                        by_zero_default = Some(val);
                        p.expect(TokenKind::Rbracket)?;
                    }
            }
            Ok(FormulaExpr::Div {
                left: Box::new(left),
                right: Box::new(right),
                by_zero_default,
            })
        }
        _ => Err(QqlError::syntax(
            alloc::format!("unknown formula operator: {}", tok.text),
            tok.pos,
        )),
    }
}

fn parse_formula_grouped_expression<'a>(p: &mut Parser<'a>) -> Result<FormulaExpr<'a>, QqlError> {
    p.advance()?;
    let expr = p.parse_formula_expr(PRECEDENCE_LOWEST)?;
    p.expect(TokenKind::Rparen)?;
    Ok(expr)
}

fn parse_formula_case_expression<'a>(p: &mut Parser<'a>) -> Result<FormulaExpr<'a>, QqlError> {
    p.advance()?;
    p.expect(TokenKind::When)?;
    let cond = p.parse_filter_expr()?;
    p.expect(TokenKind::Then)?;
    let then_expr = p.parse_formula_expr(PRECEDENCE_LOWEST)?;
    p.expect(TokenKind::Else)?;
    let else_expr = p.parse_formula_expr(PRECEDENCE_LOWEST)?;
    p.expect(TokenKind::End)?;
    Ok(FormulaExpr::Case {
        cond: Box::new(cond),
        then_: Box::new(then_expr),
        else_: Box::new(else_expr),
    })
}

// ── Function call dispatcher ────────────────────────────────────

fn parse_formula_function_call<'a>(
    p: &mut Parser<'a>,
    func_name: &str,
    pos: usize,
) -> Result<FormulaExpr<'a>, QqlError> {
    match func_name {
        "match" | "match_any" => {
            let field_tok = p.expect(TokenKind::Identifier)?;
            p.expect(TokenKind::Comma)?;
            let values = if p.peek()?.kind == TokenKind::Lbracket {
                p.parse_list()?
            } else {
                let single = p.parse_value()?;
                vec![single]
            };
            p.expect(TokenKind::Rparen)?;
            return Ok(FormulaExpr::MatchCondition {
                field: field_tok.text,
                values,
            });
        }
        "datetime" => {
            let tok = p.expect(TokenKind::String)?;
            p.expect(TokenKind::Rparen)?;
            return Ok(FormulaExpr::Datetime { value: tok.text });
        }
        "datetime_key" => {
            let tok = p.expect(TokenKind::String)?;
            p.expect(TokenKind::Rparen)?;
            return Ok(FormulaExpr::DatetimeKey { key: tok.text });
        }
        "geo_distance" if p.peek()?.kind == TokenKind::Lbrace => {
            let dict = p.parse_payload_dict()?;
            if p.peek()?.kind == TokenKind::Comma {
                p.advance()?;
            }
            let field_tok = p.expect(TokenKind::Identifier)?;
            p.expect(TokenKind::Rparen)?;

            let mut lat = None;
            let mut lon = None;
            for (k, v) in &dict {
                if ascii_equal(k, "lat") || ascii_equal(k, "LAT") {
                    match v {
                        Value::Float(f) => lat = Some(*f),
                        Value::Int(i) => lat = Some(*i as f64),
                        _ => {}
                    }
                }
                if ascii_equal(k, "lon") || ascii_equal(k, "LON") {
                    match v {
                        Value::Float(f) => lon = Some(*f),
                        Value::Int(i) => lon = Some(*i as f64),
                        _ => {}
                    }
                }
            }
            let lat =
                lat.ok_or_else(|| QqlError::syntax("geo_distance dict must have 'lat' key", pos))?;
            let lon =
                lon.ok_or_else(|| QqlError::syntax("geo_distance dict must have 'lon' key", pos))?;
            return Ok(FormulaExpr::GeoDistance {
                lat,
                lon,
                field: field_tok.text,
            });
        }
        _ => {}
    }

    let (args, kwargs) = parse_formula_call_arguments_and_kwargs(p)?;

    match func_name {
        "abs" => {
            if args.len() != 1 {
                return Err(QqlError::syntax("ABS() expects 1 argument", pos));
            }
            Ok(FormulaExpr::Abs {
                x: Box::new(args[0].clone()),
            })
        }
        "sqrt" => {
            if args.len() != 1 {
                return Err(QqlError::syntax("SQRT() expects 1 argument", pos));
            }
            Ok(FormulaExpr::Sqrt {
                x: Box::new(args[0].clone()),
            })
        }
        "log" => {
            if args.len() != 1 {
                return Err(QqlError::syntax("LOG() expects 1 argument", pos));
            }
            Ok(FormulaExpr::Log {
                x: Box::new(args[0].clone()),
            })
        }
        "ln" => {
            if args.len() != 1 {
                return Err(QqlError::syntax("LN() expects 1 argument", pos));
            }
            Ok(FormulaExpr::Ln {
                x: Box::new(args[0].clone()),
            })
        }
        "exp" => {
            if args.len() != 1 {
                return Err(QqlError::syntax("EXP() expects 1 argument", pos));
            }
            Ok(FormulaExpr::Exp {
                x: Box::new(args[0].clone()),
            })
        }
        "pow" => {
            if args.len() != 2 {
                return Err(QqlError::syntax("POW() expects 2 arguments", pos));
            }
            Ok(FormulaExpr::Pow {
                base: Box::new(args[0].clone()),
                exponent: Box::new(args[1].clone()),
            })
        }
        "geo_distance" => {
            if args.len() != 3 {
                return Err(QqlError::syntax(
                    "GEO_DISTANCE() expects 3 arguments (lat, lon, field_name)",
                    pos,
                ));
            }
            let lat = match &args[0] {
                FormulaExpr::Constant { value } => *value,
                _ => {
                    return Err(QqlError::syntax(
                        "GEO_DISTANCE() first argument must be a float constant",
                        pos,
                    ));
                }
            };
            let lon = match &args[1] {
                FormulaExpr::Constant { value } => *value,
                _ => {
                    return Err(QqlError::syntax(
                        "GEO_DISTANCE() second argument must be a float constant",
                        pos,
                    ));
                }
            };
            let field = match &args[2] {
                FormulaExpr::Variable { name } => *name,
                _ => {
                    return Err(QqlError::syntax(
                        "GEO_DISTANCE() third argument must be a field name",
                        pos,
                    ));
                }
            };
            Ok(FormulaExpr::GeoDistance { lat, lon, field })
        }
        "exp_decay" | "gauss_decay" | "lin_decay" => {
            if args.is_empty() && kwargs.is_empty() {
                return Err(QqlError::syntax(
                    alloc::format!(
                        "{}() expects at least 1 argument (x)",
                        func_name.to_uppercase()
                    ),
                    pos,
                ));
            }

            let x = if !args.is_empty() {
                args[0].clone()
            } else if let Some(val) = kwargs.iter().find(|(k, _)| *k == "x").map(|(_, v)| v) {
                val.clone()
            } else {
                return Err(QqlError::syntax(
                    alloc::format!("{}() requires 'x' argument", func_name.to_uppercase()),
                    pos,
                ));
            };

            let target = if args.len() > 1 {
                Some(Box::new(args[1].clone()))
            } else {
                kwargs
                    .iter()
                    .find(|(k, _)| *k == "target")
                    .map(|(_, v)| v)
                    .map(|val| Box::new(val.clone()))
            };

            let scale = if args.len() > 2 {
                match &args[2] {
                    FormulaExpr::Constant { value } => Some(*value),
                    _ => {
                        return Err(QqlError::syntax(
                            "scale argument in decay function must be a constant",
                            pos,
                        ));
                    }
                }
            } else if let Some(val) = kwargs.iter().find(|(k, _)| *k == "scale").map(|(_, v)| v) {
                match val {
                    FormulaExpr::Constant { value } => Some(*value),
                    _ => {
                        return Err(QqlError::syntax(
                            "scale argument in decay function must be a constant",
                            pos,
                        ));
                    }
                }
            } else {
                None
            };

            let midpoint = if args.len() > 3 {
                match &args[3] {
                    FormulaExpr::Constant { value } => Some(*value),
                    _ => {
                        return Err(QqlError::syntax(
                            "midpoint/decay argument in decay function must be a constant",
                            pos,
                        ));
                    }
                }
            } else if let Some(val) = kwargs
                .iter()
                .find(|(k, _)| *k == "midpoint")
                .map(|(_, v)| v)
            {
                match val {
                    FormulaExpr::Constant { value } => Some(*value),
                    _ => {
                        return Err(QqlError::syntax(
                            "midpoint argument in decay function must be a constant",
                            pos,
                        ));
                    }
                }
            } else if let Some(val) = kwargs.iter().find(|(k, _)| *k == "decay").map(|(_, v)| v) {
                match val {
                    FormulaExpr::Constant { value } => Some(*value),
                    _ => {
                        return Err(QqlError::syntax(
                            "decay argument in decay function must be a constant",
                            pos,
                        ));
                    }
                }
            } else {
                None
            };

            let static_kind: &'static str = match func_name {
                "exp_decay" => "exp_decay",
                "gauss_decay" => "gauss_decay",
                "lin_decay" => "lin_decay",
                _ => {
                    return Err(QqlError::syntax(
                        alloc::format!("unknown decay function: {}", func_name),
                        pos,
                    ))
                }
            };
            Ok(FormulaExpr::Decay {
                kind: static_kind,
                x: Box::new(x),
                target,
                scale,
                midpoint,
            })
        }
        _ => Err(QqlError::syntax(
            alloc::format!("unknown formula function: {}", func_name),
            pos,
        )),
    }
}

type FormulaArgs<'a> = (Vec<FormulaExpr<'a>>, Vec<(&'a str, FormulaExpr<'a>)>);

// ── Argument parsing (standalone function) ──────────────────────

fn parse_formula_call_arguments_and_kwargs<'a>(
    p: &mut Parser<'a>,
) -> Result<FormulaArgs<'a>, QqlError> {
    let mut args = Vec::new();
    let mut kwargs = Vec::new();

    if p.peek()?.kind == TokenKind::Rparen {
        p.advance()?;
        return Ok((args, kwargs));
    }

    loop {
        let is_kwarg = {
            if p.index < p.tokens.len() {
                let t = &p.tokens[p.index];
                if t.kind == TokenKind::Rparen {
                    break;
                }
                if p.index + 1 < p.tokens.len() {
                    let next = &p.tokens[p.index + 1];
                    next.kind == TokenKind::Equals
                        && (t.kind == TokenKind::Identifier || t.kind == TokenKind::String)
                } else {
                    false
                }
            } else {
                false
            }
        };

        if is_kwarg {
            let key_tok = p.advance()?;
            let _eq = p.advance()?;
            let arg = p.parse_formula_expr(PRECEDENCE_LOWEST)?;
            kwargs.push((key_tok.text, arg));
        } else {
            if !kwargs.is_empty() {
                return Err(QqlError::syntax(
                    "positional argument cannot follow keyword argument",
                    p.peek()?.pos,
                ));
            }
            let arg = p.parse_formula_expr(PRECEDENCE_LOWEST)?;
            args.push(arg);
        }

        if p.peek()?.kind == TokenKind::Comma {
            p.advance()?;
        } else {
            break;
        }
    }

    p.expect(TokenKind::Rparen)?;
    Ok((args, kwargs))
}
