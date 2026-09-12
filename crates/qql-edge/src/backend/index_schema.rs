//! `CREATE INDEX` schema lowering onto qdrant-edge `PayloadFieldSchema`.
//!
//! Empty `WITH (…)` stays a bare field type. Any option is lowered onto
//! `PayloadSchemaParams` for that type; a key that type does not accept
//! fails closed instead of being dropped.

use serde::Serialize;
use serde::de::DeserializeOwned;

use qdrant_edge::{
    BoolIndexParams, DatetimeIndexParams, FloatIndexParams, GeoIndexParams, IntegerIndexParams,
    KeywordIndexParams, PayloadFieldSchema, PayloadSchemaParams, PayloadSchemaType,
    TextIndexParams, TokenizerType, UuidIndexParams,
};

use qql_core::error::QqlError;
use qql_plan::{CreateIndexRequest, IndexFieldType, IndexOptions, TextTokenizer};

use super::config_builder::edge_memory;

pub(crate) fn edge_payload_field_schema(
    request: &CreateIndexRequest,
) -> Result<PayloadFieldSchema, QqlError> {
    let schema_type = match request.field_schema {
        IndexFieldType::Keyword => PayloadSchemaType::Keyword,
        IndexFieldType::Uuid => PayloadSchemaType::Uuid,
        IndexFieldType::Integer => PayloadSchemaType::Integer,
        IndexFieldType::Float => PayloadSchemaType::Float,
        IndexFieldType::Bool => PayloadSchemaType::Bool,
        IndexFieldType::Geo => PayloadSchemaType::Geo,
        IndexFieldType::Text => PayloadSchemaType::Text,
        IndexFieldType::Datetime => PayloadSchemaType::Datetime,
    };
    if request.options.is_empty() {
        return Ok(PayloadFieldSchema::FieldType(schema_type));
    }
    reject_incompatible_index_options(request.field_schema, &request.options)?;
    Ok(PayloadFieldSchema::FieldParams(index_params(
        request.field_schema,
        &request.options,
    )?))
}

#[allow(deprecated)]
fn index_params(
    field: IndexFieldType,
    options: &IndexOptions,
) -> Result<PayloadSchemaParams, QqlError> {
    let memory = options.memory.map(edge_memory).transpose()?;
    Ok(match field {
        IndexFieldType::Keyword => PayloadSchemaParams::Keyword(KeywordIndexParams {
            is_tenant: options.is_tenant,
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            prefix: options.prefix,
            ..Default::default()
        }),
        IndexFieldType::Uuid => PayloadSchemaParams::Uuid(UuidIndexParams {
            is_tenant: options.is_tenant,
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
        IndexFieldType::Integer => PayloadSchemaParams::Integer(IntegerIndexParams {
            lookup: options.lookup,
            range: options.range,
            is_principal: options.is_principal,
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
        IndexFieldType::Float => PayloadSchemaParams::Float(FloatIndexParams {
            is_principal: options.is_principal,
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
        IndexFieldType::Datetime => PayloadSchemaParams::Datetime(DatetimeIndexParams {
            is_principal: options.is_principal,
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
        IndexFieldType::Geo => PayloadSchemaParams::Geo(GeoIndexParams {
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
        IndexFieldType::Bool => PayloadSchemaParams::Bool(BoolIndexParams {
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
        IndexFieldType::Text => PayloadSchemaParams::Text(TextIndexParams {
            tokenizer: options
                .tokenizer
                .map(edge_tokenizer)
                .unwrap_or(TokenizerType::Word),
            min_token_len: options
                .min_token_len
                .map(usize::try_from)
                .transpose()
                .map_err(|error| {
                    index_option_error(format!("min_token_len is too large: {error}"))
                })?,
            max_token_len: options
                .max_token_len
                .map(usize::try_from)
                .transpose()
                .map_err(|error| {
                    index_option_error(format!("max_token_len is too large: {error}"))
                })?,
            lowercase: options.lowercase,
            ascii_folding: options.ascii_folding,
            phrase_matching: options.phrase_matching,
            stopwords: options
                .stopwords
                .as_ref()
                .map(|set| engine_fragment("stopwords", set))
                .transpose()?,
            stemmer: options
                .stemmer
                .as_ref()
                .map(|algorithm| engine_fragment("stemmer", algorithm))
                .transpose()?,
            on_disk: options.on_disk,
            memory,
            enable_hnsw: options.enable_hnsw,
            ..Default::default()
        }),
    })
}

/// Lower one qql-plan config fragment into the engine field type.
///
/// qdrant-edge 0.8 re-exports `SnowballLanguage` / `SnowballParams` /
/// `StopwordsSet` / `Language`, but not the wrapper enums that the
/// `TextIndexParams` fields require: `StemmingAlgorithm` and
/// `StopwordsInterface`. Neither wrapper has a public constructor or a
/// `From`/`Into` bridge, so a serde hop over the single fragment is the only
/// way to build the typed field (the plan side stays typed; no untyped body
/// is forwarded to the engine).
fn engine_fragment<T, S>(field: &str, source: &S) -> Result<T, QqlError>
where
    T: DeserializeOwned,
    S: Serialize,
{
    let value = serde_json::to_value(source).map_err(|error| {
        index_option_error(format!(
            "text index {field} could not be serialized: {error}"
        ))
    })?;
    serde_json::from_value(value).map_err(|error| {
        index_option_error(format!("text index {field} is not supported: {error}"))
    })
}

fn edge_tokenizer(tokenizer: TextTokenizer) -> TokenizerType {
    match tokenizer {
        TextTokenizer::Prefix => TokenizerType::Prefix,
        TextTokenizer::Whitespace => TokenizerType::Whitespace,
        TextTokenizer::Word => TokenizerType::Word,
        TextTokenizer::Multilingual => TokenizerType::Multilingual,
    }
}

fn reject_incompatible_index_options(
    field: IndexFieldType,
    options: &IndexOptions,
) -> Result<(), QqlError> {
    let mut unknown = Vec::new();
    let allow_tenant = matches!(field, IndexFieldType::Keyword | IndexFieldType::Uuid);
    let allow_prefix = matches!(field, IndexFieldType::Keyword);
    let allow_lookup_range = matches!(field, IndexFieldType::Integer);
    let allow_principal = matches!(
        field,
        IndexFieldType::Integer | IndexFieldType::Float | IndexFieldType::Datetime
    );
    let allow_text = matches!(field, IndexFieldType::Text);
    if options.is_tenant.is_some() && !allow_tenant {
        unknown.push("is_tenant");
    }
    if options.prefix.is_some() && !allow_prefix {
        unknown.push("prefix");
    }
    if (options.lookup.is_some() || options.range.is_some()) && !allow_lookup_range {
        unknown.push("lookup/range");
    }
    if options.is_principal.is_some() && !allow_principal {
        unknown.push("is_principal");
    }
    if !allow_text
        && (options.lowercase.is_some()
            || options.ascii_folding.is_some()
            || options.phrase_matching.is_some()
            || options.min_token_len.is_some()
            || options.max_token_len.is_some()
            || options.tokenizer.is_some()
            || options.stemmer.is_some()
            || options.stopwords.is_some())
    {
        unknown.push("text tokenizer options");
    }
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(index_option_error(format!(
            "CREATE INDEX TYPE {} does not accept {}",
            field.as_str(),
            unknown.join(", ")
        )))
    }
}

fn index_option_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_plan::IndexOptions;

    fn request(field: IndexFieldType, options: IndexOptions) -> CreateIndexRequest {
        CreateIndexRequest {
            field_name: "tenant".into(),
            field_schema: field,
            options,
        }
    }

    #[test]
    fn empty_options_are_a_bare_field_type() {
        let schema =
            edge_payload_field_schema(&request(IndexFieldType::Keyword, IndexOptions::default()))
                .expect("empty options");
        assert!(matches!(
            schema,
            PayloadFieldSchema::FieldType(PayloadSchemaType::Keyword)
        ));
    }

    #[test]
    fn keyword_tenant_option_is_lowered() {
        let schema = edge_payload_field_schema(&request(
            IndexFieldType::Keyword,
            IndexOptions {
                is_tenant: Some(true),
                ..Default::default()
            },
        ))
        .expect("tenant option");
        match schema {
            PayloadFieldSchema::FieldParams(PayloadSchemaParams::Keyword(params)) => {
                assert_eq!(params.is_tenant, Some(true));
            }
            other => panic!("expected keyword params, got {other:?}"),
        }
    }

    #[test]
    fn tokenizer_on_keyword_fails_closed() {
        let error = edge_payload_field_schema(&request(
            IndexFieldType::Keyword,
            IndexOptions {
                tokenizer: Some(TextTokenizer::Word),
                ..Default::default()
            },
        ))
        .expect_err("tokenizer is text-only");
        assert_eq!(error.code, "QQL-EDGE-CONFIG");
        assert!(
            error.message.contains("text tokenizer"),
            "{}",
            error.message
        );
    }

    /// `StemmingAlgorithm`/`StopwordsInterface` are not re-exported, but the
    /// plan fragments reach the typed `TextIndexParams` fields through serde.
    #[test]
    fn text_stemmer_and_stopwords_are_lowered() {
        let schema = edge_payload_field_schema(&request(
            IndexFieldType::Text,
            IndexOptions {
                stemmer: Some(qql_plan::StemmingAlgorithm::parse("English")),
                stopwords: Some(qql_plan::StopwordsSet {
                    languages: Vec::new(),
                    custom: vec!["the".into(), "a".into()],
                }),
                ..Default::default()
            },
        ))
        .expect("serde-reachable stemmer/stopwords");
        match schema {
            PayloadFieldSchema::FieldParams(PayloadSchemaParams::Text(params)) => {
                let value = serde_json::to_value(&params).expect("text params serialize");
                assert_eq!(value["stemmer"]["type"], "snowball");
                assert_eq!(value["stemmer"]["language"], "english");
                assert_eq!(
                    value["stopwords"]["custom"],
                    serde_json::json!(["a", "the"])
                );
            }
            other => panic!("expected text params, got {other:?}"),
        }
    }

    /// `stemmer = 'none'` maps to the engine's explicit opt-out shape.
    #[test]
    fn text_stemmer_disabled_is_lowered() {
        let schema = edge_payload_field_schema(&request(
            IndexFieldType::Text,
            IndexOptions {
                stemmer: Some(qql_plan::StemmingAlgorithm::parse("none")),
                ..Default::default()
            },
        ))
        .expect("disabled stemmer");
        match schema {
            PayloadFieldSchema::FieldParams(PayloadSchemaParams::Text(params)) => {
                let value = serde_json::to_value(&params).expect("text params serialize");
                assert_eq!(value["stemmer"]["type"], "none");
            }
            other => panic!("expected text params, got {other:?}"),
        }
    }

    /// A language the engine does not know still fails closed.
    #[test]
    fn unknown_stemmer_language_fails_closed() {
        let error = edge_payload_field_schema(&request(
            IndexFieldType::Text,
            IndexOptions {
                stemmer: Some(qql_plan::StemmingAlgorithm::Snowball("klingon".into())),
                ..Default::default()
            },
        ))
        .expect_err("unknown snowball language");
        assert_eq!(error.code, "QQL-EDGE-CONFIG");
        assert!(error.message.contains("stemmer"), "{}", error.message);
    }
}
