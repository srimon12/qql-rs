//! Decode OpenAPI vector / point-id / shard-key shapes into typed AST values.
//!
//! These primitives are shared by queries, mutations, prefetches, and DDL.

use serde_json::Value;

use crate::ConvertError;
use crate::json::{self, child, index, invalid, type_name};
use qql_core::ast::{LookupSpec, PointId, PointVectors, QueryInput, ShardKey, VectorValue};

/// Decode an `ExtendedPointId` (unsigned integer or string).
pub(crate) fn point_id(value: &Value, path: &str) -> Result<PointId, ConvertError> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .map(PointId::Number)
            .ok_or_else(|| invalid(path, "point id number must be an unsigned 64-bit integer")),
        Value::String(s) => Ok(PointId::String(s.clone())),
        other => Err(invalid(
            path,
            format!(
                "expected a point id (integer or string), got {}",
                type_name(other)
            ),
        )),
    }
}

/// Decode the optional `shard_key` request member.
pub(crate) fn shard_key_field(
    obj: &json::Obj,
    path: &str,
) -> Result<Option<ShardKey>, ConvertError> {
    match obj.get("shard_key").filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(value) => Ok(Some(shard_key(value, &child(path, "shard_key"))?)),
    }
}

/// Decode a `ShardKeySelector`.
///
/// QQL routes through exactly one typed shard key, so a single key (or a
/// one-element array) is accepted; multi-key arrays and the
/// `{target, fallback}` selector have no QQL form and fail closed.
pub(crate) fn shard_key(value: &Value, path: &str) -> Result<ShardKey, ConvertError> {
    match value {
        Value::String(s) => Ok(ShardKey::Keyword(s.clone())),
        Value::Number(n) => n
            .as_u64()
            .map(ShardKey::Number)
            .ok_or_else(|| invalid(path, "shard key number must be an unsigned 64-bit integer")),
        Value::Array(items) if items.len() == 1 => shard_key(&items[0], &index(path, 0)),
        Value::Array(_) => Err(invalid(
            path,
            "multi-key shard selectors have no QQL representation (QQL routes through one SHARD key)",
        )),
        Value::Object(_) => Err(invalid(
            path,
            "shard-key fallback selectors have no QQL representation",
        )),
        other => Err(invalid(
            path,
            format!("expected a shard key, got {}", type_name(other)),
        )),
    }
}

/// Decode a `LookupLocation` into a prefetch `LOOKUP FROM` spec.
pub(crate) fn lookup_spec(value: &Value, path: &str) -> Result<LookupSpec, ConvertError> {
    let obj = json::object(value, path)?;
    json::reject_unknown(obj, path, &["collection", "vector", "shard_key"])?;
    let collection = json::required(obj, "collection", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "collection"))?.to_string()))?;
    Ok(LookupSpec {
        collection,
        vector: json::opt_string(obj, "vector", path)?,
        shard_key: match obj.get("shard_key").filter(|v| !v.is_null()) {
            None => None,
            Some(raw) => Some(shard_key(raw, &child(path, "shard_key"))?),
        },
    })
}

/// Decode one finite JSON number as `f32`, failing on overflow.
fn f32_at(value: &Value, path: &str) -> Result<f32, ConvertError> {
    let n = json::f64_at(value, path)?;
    let narrowed = n as f32;
    if narrowed.is_finite() {
        Ok(narrowed)
    } else {
        Err(invalid(path, "number is outside the f32 range"))
    }
}

/// Decode a numeric array (`[number, …]`), empty or not.
fn dense_numbers(value: &Value, path: &str) -> Result<Vec<f32>, ConvertError> {
    json::array(value, path)?
        .iter()
        .enumerate()
        .map(|(i, item)| f32_at(item, &index(path, i)))
        .collect()
}

/// Decode a dense vector array (`[number, …]`).
///
/// QQL rejects empty dense vectors (`QQL-VALIDATION-VECTOR`), so an empty
/// array has no emitted-QQL representation and fails closed here.
fn dense(value: &Value, path: &str) -> Result<Vec<f32>, ConvertError> {
    let values = dense_numbers(value, path)?;
    if values.is_empty() {
        return Err(invalid(path, "dense vector must not be empty"));
    }
    Ok(values)
}

/// Decode a multivector array (`[[number, …], …]`).
///
/// Mirrors the parser: the outer list and every row must be non-empty.
fn multi_dense(value: &Value, path: &str) -> Result<Vec<Vec<f32>>, ConvertError> {
    let rows = json::array(value, path)?;
    if rows.is_empty() {
        return Err(invalid(path, "multidense vector must not be empty"));
    }
    let mut decoded = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let row_path = index(path, i);
        let values = dense_numbers(row, &row_path)?;
        if values.is_empty() {
            return Err(invalid(
                row_path,
                "multidense vector rows must not be empty",
            ));
        }
        decoded.push(values);
    }
    Ok(decoded)
}

/// Decode a `SparseVector` object (`{indices, values}`).
pub(crate) fn sparse(value: &Value, path: &str) -> Result<VectorValue, ConvertError> {
    let obj = json::object(value, path)?;
    // Inference keys never belong on a sparse vector; without this a
    // `{text, indices, values}` mix would silently decode as sparse while
    // the parser fails it closed.
    for key in ["text", "image", "object"] {
        if obj.contains_key(key) {
            return Err(invalid(
                child(path, key),
                "sparse vector must only carry indices and values",
            ));
        }
    }
    let indices = json::required(obj, "indices", path).and_then(|v| {
        json::array(v, &child(path, "indices"))?
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let raw = json::u64_at(item, &index(&child(path, "indices"), i))?;
                u32::try_from(raw)
                    .map_err(|_| invalid(index(&child(path, "indices"), i), "index exceeds u32"))
            })
            .collect::<Result<Vec<u32>, ConvertError>>()
    })?;
    let values = json::required(obj, "values", path)
        .and_then(|v| dense_numbers(v, &child(path, "values")))?;
    // Mirrors the parser: sparse indices and values must be non-empty and
    // equal length (`QQL-VALIDATION-VECTOR`).
    if indices.is_empty() || indices.len() != values.len() {
        return Err(invalid(
            path,
            format!(
                "sparse vector indices and values must be non-empty and have equal length ({} vs {})",
                indices.len(),
                values.len()
            ),
        ));
    }
    Ok(VectorValue::Sparse { indices, values })
}

/// Decode a `Vector` value: dense, multi-dense, sparse, or per-point
/// inference (`Document` / `Image` / `InferenceObject`).
pub(crate) fn vector_value(value: &Value, path: &str) -> Result<VectorValue, ConvertError> {
    match value {
        Value::Array(items) => {
            // Mirror the parser's discrimination: an empty array is the
            // degenerate multidense case (`QQL-VALIDATION-VECTOR`).
            if items.is_empty() || items.first().is_some_and(Value::is_array) {
                Ok(VectorValue::MultiDense(multi_dense(value, path)?))
            } else {
                Ok(VectorValue::Dense(dense(value, path)?))
            }
        }
        Value::Object(obj) => {
            if obj.contains_key("indices") || obj.contains_key("values") {
                return sparse(value, path);
            }
            if obj.contains_key("text") || obj.contains_key("image") || obj.contains_key("object") {
                return inference_vector_value(obj, path);
            }
            Err(invalid(
                path,
                format!(
                    "expected a vector (array, sparse object, or inference object), got {}",
                    type_name(value)
                ),
            ))
        }
        other => Err(invalid(
            path,
            format!(
                "expected a vector (array, sparse object, or inference object), got {}",
                type_name(other)
            ),
        )),
    }
}

/// Decode a `VectorInput`: point id, vector, document, or image.
pub(crate) fn query_input(value: &Value, path: &str) -> Result<QueryInput, ConvertError> {
    match value {
        Value::Number(_) | Value::String(_) => Ok(QueryInput::Point(point_id(value, path)?)),
        Value::Array(_) => Ok(QueryInput::Vector(vector_value(value, path)?)),
        Value::Object(obj) => {
            if obj.contains_key("indices") || obj.contains_key("values") {
                return Ok(QueryInput::Vector(sparse(value, path)?));
            }
            if obj.contains_key("text") {
                return document(obj, path);
            }
            if obj.contains_key("image") {
                return image(obj, path);
            }
            if obj.contains_key("object") {
                return inference_object(obj, path);
            }
            Err(invalid(
                path,
                "expected a VectorInput (vector, point id, text document, or image)",
            ))
        }
        other => Err(invalid(
            path,
            format!("expected a VectorInput, got {}", type_name(other)),
        )),
    }
}

/// OpenAPI `Document`: `{text, model, options?}` (model-less plans use `""`).
fn document(obj: &json::Obj, path: &str) -> Result<QueryInput, ConvertError> {
    json::reject_unknown(obj, path, &["text", "model", "options"])?;
    let text = json::required(obj, "text", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "text"))?.to_string()))?;
    let model = json::required(obj, "model", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "model"))?.to_string()))?;
    Ok(QueryInput::Text {
        text,
        model: if model.is_empty() { None } else { Some(model) },
        text_param: None,
        options: inference_options(obj, path)?,
    })
}

/// OpenAPI `Image`: `{image, model, options?}`.
fn image(obj: &json::Obj, path: &str) -> Result<QueryInput, ConvertError> {
    json::reject_unknown(obj, path, &["image", "model", "options"])?;
    let source = json::required(obj, "image", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "image"))?.to_string()))?;
    let model = json::required(obj, "model", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "model"))?.to_string()))?;
    Ok(QueryInput::Image {
        source,
        model: if model.is_empty() { None } else { Some(model) },
        options: inference_options(obj, path)?,
    })
}

/// OpenAPI `InferenceObject`: `{object, model, options?}`.
fn inference_object(obj: &json::Obj, path: &str) -> Result<QueryInput, ConvertError> {
    json::reject_unknown(obj, path, &["object", "model", "options"])?;
    let object = crate::json::json_to_ast_value(
        json::required(obj, "object", path)?,
        &child(path, "object"),
    )?;
    let model = json::required(obj, "model", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "model"))?.to_string()))?;
    Ok(QueryInput::Object {
        object: Box::new(object),
        model: if model.is_empty() { None } else { Some(model) },
        options: inference_options(obj, path)?,
    })
}

/// Decode the optional free-form `options` member of an inference input into
/// ordered AST pairs (sorted for canonical output).
fn inference_options(
    obj: &json::Obj,
    path: &str,
) -> Result<Vec<(String, qql_core::ast::Value)>, ConvertError> {
    let Some(options) = obj.get("options").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let options_path = child(path, "options");
    let map = json::object(options, &options_path)?;
    let mut pairs = Vec::with_capacity(map.len());
    for (key, value) in map {
        pairs.push((
            key.clone(),
            crate::json::json_to_ast_value(value, &child(&options_path, key))?,
        ));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(pairs)
}

/// Decode a `VectorStruct` (point-insert/update vectors) into `PointVectors`.
///
/// Unnamed dense, multi-dense, and sparse values are supported, as is a map of
/// named vectors. Per-point inference inputs (`Document` / `Image` /
/// `InferenceObject`) decode to inference vector values, whole or per name.
pub(crate) fn point_vectors(value: &Value, path: &str) -> Result<PointVectors, ConvertError> {
    match value {
        Value::Array(_) => Ok(PointVectors::Unnamed(vector_value(value, path)?)),
        Value::Object(obj) => {
            if obj.contains_key("indices") || obj.contains_key("values") {
                return Ok(PointVectors::Unnamed(sparse(value, path)?));
            }
            if obj.contains_key("text") || obj.contains_key("image") || obj.contains_key("object") {
                return Ok(PointVectors::Unnamed(inference_vector_value(obj, path)?));
            }
            let mut named = Vec::with_capacity(obj.len());
            for (name, item) in obj {
                named.push((name.clone(), named_vector_value(item, &child(path, name))?));
            }
            if named.is_empty() {
                return Err(invalid(path, "vector map must not be empty"));
            }
            Ok(PointVectors::Named(named))
        }
        other => Err(invalid(
            path,
            format!(
                "expected vectors (array or name map), got {}",
                type_name(other)
            ),
        )),
    }
}

/// Decode one named-map vector entry: dense, multi-dense, sparse, or
/// per-name inference.
fn named_vector_value(value: &Value, path: &str) -> Result<VectorValue, ConvertError> {
    vector_value(value, path).map_err(|err| {
        if let ConvertError::InvalidField { path, detail } = err {
            ConvertError::invalid(path, detail)
        } else {
            err
        }
    })
}

/// Decode an inference object (`Document` / `Image` / `InferenceObject`)
/// into a [`VectorValue`].
fn inference_vector_value(obj: &json::Obj, path: &str) -> Result<VectorValue, ConvertError> {
    let kinds = ["text", "image", "object"]
        .into_iter()
        .filter(|key| obj.contains_key(*key))
        .collect::<Vec<_>>();
    let [kind] = kinds.as_slice() else {
        return Err(invalid(
            path,
            "inference vector carries more than one of text, image, object",
        ));
    };
    json::reject_unknown(obj, path, &["text", "image", "object", "model", "options"])?;
    let model = json::string_at(json::required(obj, "model", path)?, &child(path, "model"))?;
    let model = if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    };
    let options = inference_options(obj, path)?;
    match *kind {
        "text" => Ok(VectorValue::Document {
            text: json::string_at(json::required(obj, "text", path)?, &child(path, "text"))?
                .to_string(),
            model,
            options,
        }),
        "image" => Ok(VectorValue::Image {
            source: json::string_at(json::required(obj, "image", path)?, &child(path, "image"))?
                .to_string(),
            model,
            options,
        }),
        _ => Ok(VectorValue::Object {
            object: Box::new(crate::json::json_to_ast_value(
                json::required(obj, "object", path)?,
                &child(path, "object"),
            )?),
            model,
            options,
        }),
    }
}
