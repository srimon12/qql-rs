use crate::ast::{FusionMethod, QueryInput};
use crate::error::{QqlError, Span};
use crate::parser::AstLowerer;
use crate::token::TokenKind;
use alloc::string::String;

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
            return Ok(QueryInput::Text {
                text,
                model,
                text_param,
            });
        }
        // IMAGE is a bare word (not reserved) — local path or URL for CLIP vision.
        if self.peek_word("IMAGE")? {
            self.advance()?;
            let source = self.parse_string()?;
            let model = self.parse_optional_model_string()?;
            return Ok(QueryInput::Image { source, model });
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
        if self.peek()?.kind == TokenKind::Lbracket {
            return self.parse_vector_value().map(QueryInput::Vector);
        }
        if self.peek_word("POINT")? {
            self.advance()?;
            return self.parse_point_id("POINT").map(QueryInput::Point);
        }
        if self.peek()?.kind == TokenKind::Colon {
            let colon_tok = self.advance()?;
            let name = self.parse_param_name()?;
            let span = Span::new(colon_tok.span.start, self.prev_span().end);
            return Ok(QueryInput::Param(name, Some(span)));
        }
        if self.peek()?.kind == TokenKind::Question {
            let q_tok = self.advance()?;
            let idx = self.next_positional_param();
            return Ok(QueryInput::PositionalParam(idx, Some(q_tok.span)));
        }
        if self.peek()?.kind == TokenKind::String {
            return self.parse_string().map(|text| QueryInput::Text {
                text,
                model: None,
                text_param: None,
            });
        }
        Err(QqlError::parse(
            "QQL-PARSE-QUERY-INPUT",
            "query input requires TEXT, IMAGE, VECTOR, POINT, or parameter",
            self.peek()?.span,
        ))
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
