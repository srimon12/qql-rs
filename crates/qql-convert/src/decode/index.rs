//! Decode `CREATE FIELD INDEX` bodies and their `field_schema` options.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::DecodeCtx;
use crate::json::{self, child, invalid, type_name};
use qql_core::ast::{CreateIndexStmt, Value as AstValue};

/// Payload schema types QQL's `CREATE INDEX … TYPE` accepts.
const INDEX_TYPES: [&str; 8] = [
    "keyword", "integer", "float", "geo", "text", "bool", "datetime", "uuid",
];

/// Decode a `PUT /collections/{c}/index` (`CreateFieldIndex`) body.
pub(crate) fn create_index(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<CreateIndexStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["field_name", "field_schema"])?;
    let field = json::required(obj, "field_name", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "field_name"))?.to_string()))?;
    let (field_type, options) = match obj.get("field_schema").filter(|v| !v.is_null()) {
        None => ("keyword".to_string(), Vec::new()),
        Some(Value::String(schema)) => (
            check_index_type(schema, &child(path, "field_schema"))?,
            Vec::new(),
        ),
        Some(schema @ Value::Object(_)) => {
            let schema_path = child(path, "field_schema");
            let schema_obj = json::object(schema, &schema_path)?;
            let raw_type = json::string_at(
                json::required(schema_obj, "type", &schema_path)?,
                &child(&schema_path, "type"),
            )?;
            let field_type = check_index_type(raw_type, &child(&schema_path, "type"))?;
            let options = index_options(schema_obj, &schema_path)?;
            (field_type, options)
        }
        Some(other) => {
            return Err(invalid(
                child(path, "field_schema"),
                format!(
                    "expected a type name or schema object, got {}",
                    type_name(other)
                ),
            ));
        }
    };
    Ok(CreateIndexStmt {
        collection: ctx.collection.to_string(),
        field,
        field_type,
        options,
        wait: ctx.opts.wait,
    })
}

/// Validate a payload schema type name.
fn check_index_type(raw: &str, path: &str) -> Result<String, ConvertError> {
    let lower = raw.to_ascii_lowercase();
    if INDEX_TYPES.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(invalid(path, format!("unknown index field type '{raw}'")))
    }
}

/// Extract `WITH (…)` index options from a schema object.
fn index_options(schema: &json::Obj, path: &str) -> Result<Vec<(String, AstValue)>, ConvertError> {
    let mut options = Vec::new();
    for (key, value) in schema {
        match key.as_str() {
            "type" => {}
            "is_tenant" | "on_disk" | "enable_hnsw" | "lowercase" | "ascii_folding"
            | "phrase_matching" | "lookup" | "range" | "is_principal" | "prefix" => {
                options.push((
                    key.clone(),
                    AstValue::Bool(json::bool_at(value, &child(path, key))?),
                ));
            }
            "min_token_len" | "max_token_len" => {
                let n = json::u64_at(value, &child(path, key))?;
                let n = i64::try_from(n).map_err(|_| {
                    invalid(child(path, key), "value exceeds the signed 64-bit range")
                })?;
                options.push((key.clone(), AstValue::Int(n)));
            }
            "memory" => {
                let raw = json::string_at(value, &child(path, key))?;
                options.push((key.clone(), AstValue::Str(raw.to_string())));
            }
            "tokenizer" => {
                let raw = json::string_at(value, &child(path, key))?;
                options.push((
                    key.clone(),
                    AstValue::Str(validate_tokenizer(raw, &child(path, key))?),
                ));
            }
            "stemmer" => {
                options.push((key.clone(), decode_stemmer(value, &child(path, key))?));
            }
            "stopwords" => {
                options.push((key.clone(), decode_stopwords(value, &child(path, key))?));
            }
            other => {
                return Err(invalid(child(path, other), "unknown field_schema option"));
            }
        }
    }
    // Canonical QQL is alphabetical. `serde_json::Map` iterates BTreeMap order
    // unless another crate in the cargo unit enables `preserve_order` (IndexMap
    // insertion order). Sort here so `qql convert` does not depend on that.
    options.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(options)
}

fn validate_tokenizer(raw: &str, path: &str) -> Result<String, ConvertError> {
    match raw.to_ascii_lowercase().as_str() {
        "word" | "whitespace" | "prefix" | "multilingual" => Ok(raw.to_ascii_lowercase()),
        _ => Err(invalid(path, format!("unknown text tokenizer '{raw}'"))),
    }
}

/// Decode `StemmingAlgorithm` into the AST's snowball-language string.
fn decode_stemmer(value: &Value, path: &str) -> Result<AstValue, ConvertError> {
    let obj = json::object(value, path)?;
    match json::string_at(json::required(obj, "type", path)?, &child(path, "type"))? {
        "none" => Ok(AstValue::Str("none".to_string())),
        "snowball" => Ok(AstValue::Str(
            json::string_at(
                json::required(obj, "language", path)?,
                &child(path, "language"),
            )?
            .to_ascii_lowercase(),
        )),
        other => Err(invalid(
            child(path, "type"),
            format!("unknown stemming algorithm '{other}'"),
        )),
    }
}

/// Decode `StopwordsInterface` into AST options: a bare language name, a
/// custom list, or a `{languages, custom}` set object.
fn decode_stopwords(value: &Value, path: &str) -> Result<AstValue, ConvertError> {
    match value {
        // A bare `Language` string selects the predefined list.
        Value::String(language) => Ok(AstValue::Str(check_language(language, path)?)),
        Value::Array(items) => items_to_strings(items, path).map(AstValue::List),
        Value::Object(obj) => {
            json::reject_unknown(obj, path, &["languages", "custom"])?;
            let languages = match obj.get("languages").filter(|v| !v.is_null()) {
                None => None,
                Some(languages) => {
                    let list_path = child(path, "languages");
                    let items = json::array(languages, &list_path)?;
                    let mut checked = Vec::with_capacity(items.len());
                    for (i, item) in items.iter().enumerate() {
                        checked.push(AstValue::Str(check_language(
                            json::string_at(item, &crate::json::index(&list_path, i))?,
                            &crate::json::index(&list_path, i),
                        )?));
                    }
                    Some(checked)
                }
            };
            let custom = match obj.get("custom").filter(|v| !v.is_null()) {
                None => None,
                Some(custom) => {
                    let list_path = child(path, "custom");
                    Some(items_to_strings(
                        json::array(custom, &list_path)?,
                        &list_path,
                    )?)
                }
            };
            // A custom-only set is the plain list form; language sets keep the
            // `{languages: […], custom: […]}` object spelling (empty sides
            // omitted so the canonical form is minimal).
            match (languages, custom) {
                (None, Some(words)) => Ok(AstValue::List(words)),
                (languages, custom) => {
                    let mut entries = Vec::new();
                    if let Some(languages) = languages.filter(|words| !words.is_empty()) {
                        entries.push(("languages".to_string(), AstValue::List(languages)));
                    }
                    if let Some(custom) = custom.filter(|words| !words.is_empty()) {
                        entries.push(("custom".to_string(), AstValue::List(custom)));
                    }
                    Ok(AstValue::Dict(entries))
                }
            }
        }
        other => Err(invalid(
            path,
            format!("expected stopwords, got {}", type_name(other)),
        )),
    }
}

/// Validate a stopword language against the OpenAPI `Language` enum,
/// normalizing to lowercase.
fn check_language(raw: &str, path: &str) -> Result<String, ConvertError> {
    let lower = raw.to_ascii_lowercase();
    if STOPWORD_LANGUAGES.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(invalid(path, format!("unknown stopwords language '{raw}'")))
    }
}

/// OpenAPI `Language` names accepted in `stopwords` language lists.
/// Mirrors the plan-side table; both reject unknown languages fail-closed.
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

/// Decode a JSON array of strings into AST string literals.
fn items_to_strings(items: &[Value], path: &str) -> Result<Vec<AstValue>, ConvertError> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            Ok(AstValue::Str(
                json::string_at(item, &crate::json::index(path, i))?.to_string(),
            ))
        })
        .collect()
}
