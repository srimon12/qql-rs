//! ALTER-only collection-config diff clauses (`AstLowerer` methods).
//!
//! Split from `config_parsers.rs` (size hygiene): the `WITH VECTOR <name>`,
//! nested `QUANTIZATION`, and `WITH SPARSE <name>` diff forms plus their
//! private helpers. The shared `WITH <block>` parsers stay in
//! `config_parsers`; this module calls back into those `pub` block parsers.
//! Behavior is unchanged.

use alloc::boxed::Box;

use crate::ast::{
    CollectionConfig, HnswRuntimeConfig, QuantizationUpdate, SparseVectorDiff, VectorDiff,
    VectorsConfig,
};
use crate::error::{QqlError, Span};
use crate::token::TokenKind;

use super::{AstLowerer, ascii_equal};

/// Malformed or unrepresentable `ALTER COLLECTION` per-vector diff.
fn vector_diff_error(message: impl Into<alloc::borrow::Cow<'static, str>>, span: Span) -> QqlError {
    QqlError::parse("QQL-PARSE-VECTOR-DIFF", message, span)
}

/// True when an HNSW block sets no field at all (`HNSW ()`).
fn hnsw_is_empty(config: &HnswRuntimeConfig) -> bool {
    config.m.is_none()
        && config.ef_construct.is_none()
        && config.full_scan_threshold.is_none()
        && config.max_indexing_threads.is_none()
        && config.on_disk.is_none()
        && config.payload_m.is_none()
        && config.inline_storage.is_none()
        && config.memory.is_none()
}

/// True when a VECTOR block sets no field at all (`VECTOR ()`).
fn vectors_config_is_empty(config: &VectorsConfig) -> bool {
    config.on_disk.is_none() && config.memory.is_none() && config.datatype.is_none()
}

impl<'a> AstLowerer<'a> {
    /// `WITH VECTOR <name> (HNSW (…), QUANTIZATION (…), VECTOR (…))` — ALTER-only.
    ///
    /// Nested blocks mirror the per-vector clauses of `CREATE COLLECTION` and
    /// lower onto the wire `VectorParamsDiff`; the unnamed `WITH VECTOR (…)`
    /// form is the default-vector diff and is handled by
    /// [`Self::parse_vectors_config_block`].
    pub(crate) fn parse_vector_diff_clause(&mut self) -> Result<CollectionConfig, QqlError> {
        let name = self.parse_identifier()?;
        let mut hnsw = None;
        let mut quantization = None;
        let mut vectors = None;

        self.expect(TokenKind::Lparen)?;
        if self.peek()?.kind == TokenKind::Rparen {
            return Err(vector_diff_error(
                alloc::format!(
                    "vector diff '{name}' requires at least one of HNSW (...), QUANTIZATION (...), or VECTOR (...)"
                ),
                self.peek()?.span,
            ));
        }
        loop {
            let tok = self.peek()?;
            match tok.kind {
                TokenKind::Hnsw => {
                    self.advance()?;
                    if hnsw.is_some() {
                        return Err(vector_diff_error(
                            alloc::format!("duplicate HNSW block in vector diff '{name}'"),
                            tok.span,
                        ));
                    }
                    hnsw = self.parse_hnsw_config_block()?.hnsw;
                    if hnsw.as_deref().is_none_or(hnsw_is_empty) {
                        return Err(vector_diff_error(
                            alloc::format!(
                                "HNSW (...) in vector diff '{name}' requires at least one setting"
                            ),
                            tok.span,
                        ));
                    }
                }
                TokenKind::Quantization => {
                    self.advance()?;
                    if quantization.is_some() {
                        return Err(vector_diff_error(
                            alloc::format!("duplicate QUANTIZATION block in vector diff '{name}'"),
                            tok.span,
                        ));
                    }
                    quantization = Some(self.parse_quantization_diff_block()?);
                }
                TokenKind::Vector => {
                    self.advance()?;
                    if vectors.is_some() {
                        return Err(vector_diff_error(
                            alloc::format!("duplicate VECTOR block in vector diff '{name}'"),
                            tok.span,
                        ));
                    }
                    let block = self.parse_vectors_config_block()?.vectors;
                    // OpenAPI `VectorParamsDiff` / proto `VectorParamsDiff` have
                    // no `datatype` field: the wire cannot express a per-vector
                    // datatype change, so fail closed instead of dropping it.
                    if block.as_deref().is_some_and(|cfg| cfg.datatype.is_some()) {
                        return Err(vector_diff_error(
                            "datatype cannot be changed per vector: the Qdrant VectorParamsDiff wire shape has no datatype field",
                            tok.span,
                        ));
                    }
                    if block.as_deref().is_none_or(vectors_config_is_empty) {
                        return Err(vector_diff_error(
                            alloc::format!(
                                "VECTOR (...) in vector diff '{name}' requires on_disk or memory"
                            ),
                            tok.span,
                        ));
                    }
                    vectors = block;
                }
                _ if tok.is_keyword_or_identifier() && ascii_equal(tok.text, "QUANTIZATION") => {
                    // Defensive mirror of the main clause dispatch (some lexer
                    // paths surface QUANTIZATION as an identifier).
                    self.advance()?;
                    if quantization.is_some() {
                        return Err(vector_diff_error(
                            alloc::format!("duplicate QUANTIZATION block in vector diff '{name}'"),
                            tok.span,
                        ));
                    }
                    quantization = Some(self.parse_quantization_diff_block()?);
                }
                _ => {
                    return Err(vector_diff_error(
                        alloc::format!(
                            "expected HNSW (...), QUANTIZATION (...), or VECTOR (...) in vector diff '{}', got '{}'",
                            name,
                            tok.text
                        ),
                        tok.span,
                    ));
                }
            }
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
            self.reject_trailing_comma(TokenKind::Rparen)?;
        }
        self.expect(TokenKind::Rparen)?;

        Ok(CollectionConfig {
            vectors: None,
            hnsw: None,
            optimizers: None,
            params: None,
            quantization: None,
            quantization_update: None,
            wal: None,
            strict_mode: None,
            metadata: None,
            vector_diffs: alloc::vec![VectorDiff {
                name,
                hnsw,
                quantization,
                vectors,
            }],
            sparse_vector_diffs: Vec::new(),
        })
    }

    /// Parse one nested `QUANTIZATION (…)` block into the ALTER diff form:
    /// `disabled = true` clears it, otherwise the config replaces it.
    pub(crate) fn parse_quantization_diff_block(
        &mut self,
    ) -> Result<Box<QuantizationUpdate>, QqlError> {
        let block = self.parse_quantization_config_block()?;
        let disabled = block
            .quantization_update
            .as_ref()
            .is_some_and(|update| update.disabled);
        let config = block
            .quantization_update
            .and_then(|update| update.config)
            .or(block.quantization);
        Ok(Box::new(QuantizationUpdate { disabled, config }))
    }

    /// `WITH SPARSE <name> (SPARSE (…) | INDEX (…))` — ALTER-only.
    pub(crate) fn parse_sparse_vector_diff_clause(&mut self) -> Result<CollectionConfig, QqlError> {
        let name = self.parse_identifier()?;
        self.expect(TokenKind::Lparen)?;
        let tok = self.peek()?;
        if !matches!(tok.kind, TokenKind::Sparse | TokenKind::Index) {
            return Err(vector_diff_error(
                alloc::format!(
                    "expected SPARSE (...) or INDEX (...) in sparse vector diff '{name}', got '{}'",
                    tok.text
                ),
                tok.span,
            ));
        }
        self.advance()?;
        let (index, modifier) = self.parse_sparse_config_block()?;
        if index.is_none() && modifier.is_none() {
            return Err(vector_diff_error(
                alloc::format!(
                    "SPARSE (...) in sparse vector diff '{name}' requires at least one setting"
                ),
                tok.span,
            ));
        }
        self.expect(TokenKind::Rparen)?;

        Ok(CollectionConfig {
            vectors: None,
            hnsw: None,
            optimizers: None,
            params: None,
            quantization: None,
            quantization_update: None,
            wal: None,
            strict_mode: None,
            metadata: None,
            vector_diffs: Vec::new(),
            sparse_vector_diffs: alloc::vec![SparseVectorDiff {
                name,
                index,
                modifier,
            }],
        })
    }
}
