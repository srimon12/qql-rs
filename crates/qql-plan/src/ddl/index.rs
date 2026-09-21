//! Index lowering: CREATE INDEX options onto the typed request.
//!
//! Pure move from `ddl.rs` (size hygiene split).

use crate::types::*;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use qql_core::ast::{CreateIndexStmt, Value};
use qql_core::error::{QqlError, Span};

/// Lower `CREATE INDEX` to the transport-neutral index request.
pub fn lower_create_index(stmt: &CreateIndexStmt) -> Result<CreateIndexRequest, QqlError> {
    let field_schema = IndexFieldType::parse(&stmt.field_type).ok_or_else(|| {
        QqlError::validation(
            "QQL-PLAN-INDEX-TYPE",
            format!("unknown index field type '{}'", stmt.field_type),
            None,
        )
    })?;

    let mut options = IndexOptions::default();
    for (key, value) in &stmt.options {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "is_tenant" => options.is_tenant = Some(index_bool(key, value)?),
            "on_disk" => options.on_disk = Some(index_bool(key, value)?),
            "enable_hnsw" => options.enable_hnsw = Some(index_bool(key, value)?),
            "lowercase" => options.lowercase = Some(index_bool(key, value)?),
            "ascii_folding" => options.ascii_folding = Some(index_bool(key, value)?),
            "phrase_matching" => options.phrase_matching = Some(index_bool(key, value)?),
            "lookup" => options.lookup = Some(index_bool(key, value)?),
            "range" => options.range = Some(index_bool(key, value)?),
            "is_principal" => options.is_principal = Some(index_bool(key, value)?),
            "prefix" => options.prefix = Some(index_bool(key, value)?),
            "min_token_len" => options.min_token_len = Some(index_u64(key, value)?),
            "max_token_len" => options.max_token_len = Some(index_u64(key, value)?),
            "tokenizer" => {
                let raw = index_str(key, value)?;
                options.tokenizer = Some(TextTokenizer::parse(raw).ok_or_else(|| {
                    QqlError::validation(
                        "QQL-PLAN-INDEX-OPTION",
                        format!(
                            "unsupported text tokenizer '{raw}'. Expected: word, whitespace, prefix, multilingual"
                        ),
                        value.param_span(),
                    )
                })?);
            }
            "stemmer" => options.stemmer = Some(StemmingAlgorithm::parse(index_str(key, value)?)),
            "stopwords" => {
                options.stopwords = Some(lower_stopwords(key, value)?);
            }
            "memory" => {
                let raw = index_str(key, value)?;
                options.memory = Some(MemoryPlacement::parse(raw).ok_or_else(|| {
                    QqlError::validation(
                        "QQL-PLAN-INDEX-OPTION",
                        format!(
                            "unsupported memory placement '{raw}'. Expected: cold, cached, pinned"
                        ),
                        value.param_span(),
                    )
                })?);
            }
            other => {
                return Err(index_option_error(
                    format!("unknown index option '{other}'"),
                    value.param_span(),
                ));
            }
        }
    }

    Ok(CreateIndexRequest {
        field_name: stmt.field.clone(),
        field_schema,
        options,
    })
}

fn index_option_error(
    message: impl Into<alloc::borrow::Cow<'static, str>>,
    span: Option<Span>,
) -> QqlError {
    QqlError::validation("QQL-PLAN-INDEX-OPTION", message, span)
}

fn index_bool(key: &str, value: &Value) -> Result<bool, QqlError> {
    match value {
        Value::Bool(value) => Ok(*value),
        _ => Err(QqlError::validation(
            "QQL-PLAN-INDEX-OPTION",
            format!("{key} must be true or false"),
            value.param_span(),
        )),
    }
}

fn index_u64(key: &str, value: &Value) -> Result<u64, QqlError> {
    match value {
        Value::Int(value) if *value >= 0 => Ok(*value as u64),
        _ => Err(QqlError::validation(
            "QQL-PLAN-INDEX-OPTION",
            format!("{key} must be a non-negative integer"),
            value.param_span(),
        )),
    }
}

fn index_str<'a>(key: &str, value: &'a Value) -> Result<&'a str, QqlError> {
    match value {
        Value::Str(value) => Ok(value),
        _ => Err(QqlError::validation(
            "QQL-PLAN-INDEX-OPTION",
            format!("{key} must be a string"),
            value.param_span(),
        )),
    }
}

fn index_str_list(key: &str, value: &Value) -> Result<Vec<String>, QqlError> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::Str(item) => Ok(item.clone()),
                _ => Err(QqlError::validation(
                    "QQL-PLAN-INDEX-OPTION",
                    format!("{key} must be a list of strings"),
                    item.param_span(),
                )),
            })
            .collect(),
        _ => Err(QqlError::validation(
            "QQL-PLAN-INDEX-OPTION",
            format!("{key} must be a list of strings"),
            value.param_span(),
        )),
    }
}

/// Lower a `stopwords` index option: a custom list, a bare language name, or
/// a `{languages: […], custom: […]}` set mirroring OpenAPI `StopwordsSet`.
fn lower_stopwords(key: &str, value: &Value) -> Result<StopwordsSet, QqlError> {
    match value {
        Value::List(_) => Ok(StopwordsSet {
            languages: Vec::new(),
            custom: index_str_list(key, value)?,
        }),
        Value::Str(language) => Ok(StopwordsSet {
            languages: vec![check_stopword_language(language, value.param_span())?],
            custom: Vec::new(),
        }),
        Value::Dict(entries) => {
            let mut languages = Vec::new();
            let mut custom = Vec::new();
            for (entry_key, entry_value) in entries {
                if entry_key.eq_ignore_ascii_case("languages") {
                    let items = match entry_value {
                        Value::List(items) => items,
                        _ => {
                            return Err(QqlError::validation(
                                "QQL-PLAN-INDEX-OPTION",
                                "stopwords languages must be a list of strings",
                                entry_value.param_span(),
                            ));
                        }
                    };
                    for item in items {
                        match item {
                            Value::Str(language) => {
                                languages
                                    .push(check_stopword_language(language, item.param_span())?);
                            }
                            _ => {
                                return Err(QqlError::validation(
                                    "QQL-PLAN-INDEX-OPTION",
                                    "stopwords languages must be a list of strings",
                                    item.param_span(),
                                ));
                            }
                        }
                    }
                } else if entry_key.eq_ignore_ascii_case("custom") {
                    match entry_value {
                        Value::List(items) => {
                            for item in items {
                                match item {
                                    Value::Str(word) => custom.push(word.clone()),
                                    _ => {
                                        return Err(QqlError::validation(
                                            "QQL-PLAN-INDEX-OPTION",
                                            "stopwords custom must be a list of strings",
                                            item.param_span(),
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(QqlError::validation(
                                "QQL-PLAN-INDEX-OPTION",
                                "stopwords custom must be a list of strings",
                                entry_value.param_span(),
                            ));
                        }
                    }
                } else {
                    return Err(index_option_error(
                        format!(
                            "unknown stopwords set key '{entry_key}'. Expected: languages, custom"
                        ),
                        entry_value.param_span(),
                    ));
                }
            }
            Ok(StopwordsSet { languages, custom })
        }
        _ => Err(QqlError::validation(
            "QQL-PLAN-INDEX-OPTION",
            format!(
                "{key} must be a list of strings, a language name, or {{languages: […], custom: […]}}"
            ),
            value.param_span(),
        )),
    }
}

/// Validate a stopword language against the OpenAPI `Language` enum
/// (case-insensitive); returns the canonical lowercase name.
fn check_stopword_language(raw: &str, span: Option<Span>) -> Result<String, QqlError> {
    let lower = raw.to_ascii_lowercase();
    if STOPWORD_LANGUAGES.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(index_option_error(
            format!("unknown stopwords language '{raw}'"),
            span,
        ))
    }
}

/// OpenAPI `Language` names accepted in `stopwords` language lists.
const STOPWORD_LANGUAGES: &[&str] = &[
    "arabic",
    "azerbaijani",
    "basque",
    "bengali",
    "catalan",
    "chinese",
    "danish",
    "dutch",
    "english",
    "finnish",
    "french",
    "german",
    "greek",
    "hebrew",
    "hinglish",
    "hungarian",
    "indonesian",
    "italian",
    "japanese",
    "kazakh",
    "nepali",
    "norwegian",
    "portuguese",
    "romanian",
    "russian",
    "slovene",
    "spanish",
    "swedish",
    "tajik",
    "turkish",
];
