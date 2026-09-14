use crate::ast::{FusionMethod, QueryInput, Value};
use crate::error::{QqlError, Span};
use crate::parser::AstLowerer;
use crate::token::TokenKind;
use alloc::string::String;
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
    pub(crate) fn parse_query_input(&mut self) -> Result<QueryInput, QqlError> {
        if self.peek_word("TEXT")? {
            self.advance()?;
            let (text, text_param) = if self.peek()?.kind == TokenKind::Colon {
                self.advance()?;
                let name = self.parse_param_name()?;
                (String::new(), Some(alloc::format!(":{}", name)))
            } else if self.peek()?.kind == TokenKind::Question {
                self.advance()?;
                let idx = self.next_positional_param();
                (String::new(), Some(alloc::format!("?{}", idx)))
            } else {
                (self.parse_string()?, None)
            };
            let model = self.parse_optional_model_string()?;
            let options = self.parse_optional_inference_options()?;
            return Ok(QueryInput::Text {
                text,
                model,
                text_param,
                options,
            });
        }
        // IMAGE is a bare word (not reserved) — local path or URL for CLIP vision.
        if self.peek_word("IMAGE")? {
            self.advance()?;
            let source = self.parse_string()?;
            let model = self.parse_optional_model_string()?;
            let options = self.parse_optional_inference_options()?;
            return Ok(QueryInput::Image {
                source,
                model,
                options,
            });
        }
        // OBJECT is the custom inference input (OpenAPI InferenceObject):
        // an arbitrary model payload with optional MODEL / OPTIONS.
        // Parentheses around the object are accepted (`OBJECT ({…})`) but
        // never emitted; the canonical form is the bare object.
        if self.peek_word("OBJECT")? {
            self.advance()?;
            let object = if self.peek()?.kind == TokenKind::Lparen {
                self.advance()?;
                let entries = self.parse_payload_dict()?;
                self.expect(TokenKind::Rparen)?;
                Value::Dict(entries)
            } else {
                Value::Dict(self.parse_payload_dict()?)
            };
            let model = self.parse_optional_model_string()?;
            let options = self.parse_optional_inference_options()?;
            return Ok(QueryInput::Object {
                object: alloc::boxed::Box::new(object),
                model,
                options,
            });
        }
        if self.peek()?.kind == TokenKind::Vector {
            self.advance()?;
            // `VECTOR :name` / `VECTOR ?` — the explicit spelling of a
            // parameter bound to the vector slot. Lower to the same
            // `Param` / `PositionalParam` node as the implicit
            // `QUERY :x USING …` form so AST binding and textual binding
            // behave identically.
            if matches!(self.peek()?.kind, TokenKind::Colon | TokenKind::Question) {
                return self.parse_query_input();
            }
            return self.parse_vector_value().map(QueryInput::Vector);
        }
        if self.peek()?.kind == TokenKind::Lbracket || self.peek()?.kind == TokenKind::Lbrace {
            return self.parse_vector_value().map(QueryInput::Vector);
        }
        if self.peek()?.kind == TokenKind::Point {
            self.advance()?;
            return self.parse_point_id("POINT").map(QueryInput::Point);
        }
        if self.peek()?.kind == TokenKind::Colon {
            let colon_tok = self.advance()?;
            let name = self.parse_param_name()?;
            let span = Span::new(colon_tok.span.start, self.prev_span().end);
            return Ok(QueryInput::Param(name, Some(alloc::boxed::Box::new(span))));
        }
        if self.peek()?.kind == TokenKind::Question {
            let q_tok = self.advance()?;
            let idx = self.next_positional_param();
            return Ok(QueryInput::PositionalParam(
                idx,
                Some(alloc::boxed::Box::new(q_tok.span)),
            ));
        }
        if self.peek()?.kind == TokenKind::String {
            return self.parse_string().map(|text| QueryInput::Text {
                text,
                model: None,
                text_param: None,
                options: Vec::new(),
            });
        }
        Err(QqlError::parse(
            "QQL-PARSE-QUERY-INPUT",
            "query input requires TEXT, IMAGE, OBJECT, VECTOR, POINT, or parameter",
            self.peek()?.span,
        ))
    }

    /// Parse an optional trailing `OPTIONS {…}` inference-options dict.
    ///
    /// The dict is opaque: keys and values pass through to the inference
    /// service as-is (including BM25 option objects). Duplicate keys are
    /// rejected by `parse_payload_dict`.
    pub(crate) fn parse_optional_inference_options(
        &mut self,
    ) -> Result<Vec<(String, Value)>, QqlError> {
        if self.peek_word("OPTIONS")? {
            self.advance()?;
            return self.parse_payload_dict();
        }
        Ok(Vec::new())
    }

    pub(crate) fn parse_fusion_method(&mut self) -> Result<FusionMethod, QqlError> {
        let token = self.advance()?;
        if token.text.eq_ignore_ascii_case("rrf") {
            Ok(FusionMethod::Rrf)
        } else if token.text.eq_ignore_ascii_case("dbsf") {
            Ok(FusionMethod::Dbsf)
        } else {
            Err(QqlError::validation(
                "QQL-VALIDATION-FUSION",
                "fusion method must be RRF or DBSF",
                Some(token.span),
            ))
        }
    }
}
