//! Decode `CREATE FIELD INDEX` bodies and their `field_schema` options.

use serde_json::Value;

use crate::ConvertError;
use crate::json::{self, child, invalid, type_name};
use qql_core::ast::{CreateIndexStmt, Value as AstValue};

/// Payload schema types QQL's `CREATE INDEX … TYPE` accepts.
const INDEX_TYPES: [&str; 8] = [
    "keyword", "integer", "float", "geo", "text", "bool", "datetime", "uuid",
];

/// Decode a `PUT /collections/{c}/index` (`CreateFieldIndex`) body.
pub(crate) fn create_index(
    body: &Value,
    collection: &str,
) -> Result<CreateIndexStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
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
        collection: collection.to_string(),
        field,
        field_type,
        options,
        wait: None,
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

/// Decode `StopwordsInterface` into the AST's custom stopword list.
fn decode_stopwords(value: &Value, path: &str) -> Result<AstValue, ConvertError> {
    match value {
        Value::Array(items) => items_to_strings(items, path).map(AstValue::List),
        Value::Object(obj) => {
            if obj.get("languages").is_some_and(|v| !v.is_null()) {
                return Err(invalid(
                    child(path, "languages"),
                    "predefined stopword languages have no QQL representation; only a custom list is supported",
                ));
            }
            match obj.get("custom").filter(|v| !v.is_null()) {
                None => Ok(AstValue::List(Vec::new())),
                Some(value) => {
                    let list_path = child(path, "custom");
                    items_to_strings(json::array(value, &list_path)?, &list_path)
                        .map(AstValue::List)
                }
            }
        }
        other => Err(invalid(
            path,
            format!("expected stopwords, got {}", type_name(other)),
        )),
    }
}

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
