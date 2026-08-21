use super::AstLowerer;
use crate::ast::{
    IdfParams, PayloadSelector, QuantizationSearchParams, ReadConsistency, SearchParams, Value,
    VectorSelector,
};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::string::String;
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
    pub fn parse_search_params(&mut self) -> Result<SearchParams, QqlError> {
        let values = self.parse_config_block()?;
        let mut params = SearchParams::default();
        for (key, value) in values {
            match key.to_ascii_lowercase().as_str() {
                "hnsw_ef" => params.hnsw_ef = Some(positive_integer(value, &key)?),
                "exact" => params.exact = Some(boolean(value, &key)?),
                "acorn" => params.acorn = Some(boolean(value, &key)?),
                "max_selectivity" => {
                    params.max_selectivity = Some(unit_interval(value, &key)?);
                }
                "indexed_only" => params.indexed_only = Some(boolean(value, &key)?),
                "rrf_k" => params.rrf_k = Some(positive_integer(value, &key)?),
                "rrf_weights" => params.rrf_weights = Some(float_list(value, &key)?),
                "quantization" => {
                    params.quantization = Some(quantization(value)?);
                }
                "idf" => params.idf = Some(idf_params(value)?),
                // OpenAPI query param / proto field — seconds, minimum 1.
                "timeout" => params.timeout = Some(positive_integer(value, &key)?),
                // OpenAPI ReadConsistency: factor N or majority|quorum|all.
                "consistency" => params.consistency = Some(read_consistency(value, &key)?),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-VALIDATION-SEARCH-PARAM",
                        alloc::format!("unknown search parameter '{}'", key),
                        None,
                    ));
                }
            }
        }
        if params.max_selectivity.is_some() && params.acorn != Some(true) {
            return Err(QqlError::validation(
                "QQL-VALIDATION-ACORN-SELECTIVITY",
                "max_selectivity requires PARAMS (acorn = true, …)",
                None,
            ));
        }
        Ok(params)
    }

    pub fn parse_payload_selector(&mut self) -> Result<PayloadSelector, QqlError> {
        if let Some(value) = self.parse_selector_bool()? {
            return Ok(if value {
                PayloadSelector::All
            } else {
                PayloadSelector::None
            });
        }
        let mode = self.parse_identifier()?;
        let fields = self.parse_name_list()?;
        if mode.eq_ignore_ascii_case("include") {
            Ok(PayloadSelector::Include(fields))
        } else if mode.eq_ignore_ascii_case("exclude") {
            Ok(PayloadSelector::Exclude(fields))
        } else {
            Err(QqlError::parse(
                "QQL-PARSE-PAYLOAD-SELECTOR",
                "WITH PAYLOAD requires true, false, INCLUDE (...), or EXCLUDE (...) ",
                self.peek()?.span,
            ))
        }
    }

    /// Parse a vector selector after `WITH VECTOR`.
    ///
    /// Accepts:
    /// - `true` / `false`
    /// - `(name, …)` named list
    /// - bare form (next token is not a selector) → [`VectorSelector::All`]
    ///
    /// Shared by QUERY and SCROLL so both accept `WITH VECTOR` without an
    /// explicit selector.
    pub fn parse_vector_selector(&mut self) -> Result<VectorSelector, QqlError> {
        if let Some(value) = self.parse_selector_bool()? {
            return Ok(if value {
                VectorSelector::All
            } else {
                VectorSelector::None
            });
        }
        if self.peek()?.kind == TokenKind::Lparen {
            return self.parse_name_list().map(VectorSelector::Names);
        }
        Ok(VectorSelector::All)
    }

    fn parse_selector_bool(&mut self) -> Result<Option<bool>, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::True
            || (tok.is_keyword_or_identifier() && tok.text.eq_ignore_ascii_case("true"))
        {
            self.advance()?;
            return Ok(Some(true));
        }
        if tok.kind == TokenKind::False
            || (tok.is_keyword_or_identifier() && tok.text.eq_ignore_ascii_case("false"))
        {
            self.advance()?;
            return Ok(Some(false));
        }
        Ok(None)
    }

    fn parse_name_list(&mut self) -> Result<Vec<String>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut names = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            return Err(QqlError::parse(
                "QQL-PARSE-SELECTOR",
                "selector list cannot be empty",
                self.peek()?.span,
            ));
        }
        loop {
            names.push(self.parse_identifier()?);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(names)
    }
}

fn positive_integer(value: Value, key: &str) -> Result<u64, QqlError> {
    match value {
        Value::Int(value) if value > 0 => Ok(value as u64),
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-SEARCH-PARAM",
            alloc::format!("{} must be a positive integer", key),
            None,
        )),
    }
}

fn boolean(value: Value, key: &str) -> Result<bool, QqlError> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-SEARCH-PARAM",
            alloc::format!("{} must be true or false", key),
            None,
        )),
    }
}

fn quantization(value: Value) -> Result<QuantizationSearchParams, QqlError> {
    let Value::Dict(values) = value else {
        return Err(QqlError::validation(
            "QQL-VALIDATION-SEARCH-PARAM",
            "quantization must be an object",
            None,
        ));
    };
    let mut params = QuantizationSearchParams::default();
    for (key, value) in values {
        match key.to_ascii_lowercase().as_str() {
            "ignore" => params.ignore = Some(boolean(value, &key)?),
            "rescore" => params.rescore = Some(boolean(value, &key)?),
            "oversampling" => {
                let value = match value {
                    Value::Int(value) => value as f64,
                    Value::Float(value) => value,
                    _ => {
                        return Err(QqlError::validation(
                            "QQL-VALIDATION-SEARCH-PARAM",
                            "oversampling must be numeric",
                            None,
                        ));
                    }
                };
                if !value.is_finite() || value <= 0.0 {
                    return Err(QqlError::validation(
                        "QQL-VALIDATION-SEARCH-PARAM",
                        "oversampling must be finite and greater than zero",
                        None,
                    ));
                }
                params.oversampling = Some(value);
            }
            _ => {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-SEARCH-PARAM",
                    alloc::format!("unknown quantization search parameter '{}'", key),
                    None,
                ));
            }
        }
    }
    Ok(params)
}

/// OpenAPI `IdfParams`: `"global"` (collection-wide) or `{"corpus": <filter>}`.
///
/// The corpus is kept as a raw QQL `Value` in Qdrant filter JSON shape and is
/// lowered to a typed plan filter by the plan layer.
fn idf_params(value: Value) -> Result<IdfParams, QqlError> {
    match value {
        Value::Str(s) if s.eq_ignore_ascii_case("global") => Ok(IdfParams { corpus: None }),
        Value::Dict(entries) => {
            let Some((_, corpus)) = entries
                .into_iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("corpus"))
            else {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-IDF",
                    "idf object must contain a 'corpus' filter",
                    None,
                ));
            };
            if !matches!(corpus, Value::Dict(_)) {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-IDF",
                    "idf corpus must be a filter object",
                    None,
                ));
            }
            Ok(IdfParams {
                corpus: Some(corpus),
            })
        }
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-IDF",
            "idf must be 'global' or an object with a 'corpus' filter",
            None,
        )),
    }
}

fn float_list(value: Value, key: &str) -> Result<Vec<f64>, QqlError> {
    let Value::List(items) = value else {
        return Err(QqlError::validation(
            "QQL-VALIDATION-SEARCH-PARAM",
            alloc::format!("{} must be a list of numbers", key),
            None,
        ));
    };
    let mut res = Vec::new();
    for item in items {
        match item {
            Value::Int(v) => res.push(v as f64),
            Value::Float(v) => res.push(v),
            _ => {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-SEARCH-PARAM",
                    alloc::format!("{} elements must be numbers", key),
                    None,
                ));
            }
        }
    }
    Ok(res)
}

/// Number in (0, 1] for ACORN max_selectivity.
fn unit_interval(value: Value, key: &str) -> Result<f64, QqlError> {
    let value = match value {
        Value::Int(v) => v as f64,
        Value::Float(v) => v,
        _ => {
            return Err(QqlError::validation(
                "QQL-VALIDATION-SEARCH-PARAM",
                alloc::format!("{key} must be a number"),
                None,
            ));
        }
    };
    if !value.is_finite() || value <= 0.0 || value > 1.0 {
        return Err(QqlError::validation(
            "QQL-VALIDATION-SEARCH-PARAM",
            alloc::format!("{key} must be a finite number in (0, 1]"),
            None,
        ));
    }
    Ok(value)
}

/// OpenAPI `ReadConsistency`: integer factor or majority|quorum|all.
fn read_consistency(value: Value, key: &str) -> Result<ReadConsistency, QqlError> {
    match value {
        Value::Int(v) if v >= 0 => Ok(ReadConsistency::Factor(v as u64)),
        Value::Str(s) => match s.to_ascii_lowercase().as_str() {
            "majority" => Ok(ReadConsistency::Majority),
            "quorum" => Ok(ReadConsistency::Quorum),
            "all" => Ok(ReadConsistency::All),
            _ => Err(QqlError::validation(
                "QQL-VALIDATION-CONSISTENCY",
                "consistency must be a non-negative integer factor, or majority|quorum|all",
                None,
            )),
        },
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-CONSISTENCY",
            alloc::format!("{key} must be a non-negative integer factor, or majority|quorum|all"),
            None,
        )),
    }
}
