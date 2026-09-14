//! Decode point mutation request bodies (upsert, delete, payload/vector writes).

use serde_json::Value;

use crate::ConvertError;
use crate::decode::DecodeCtx;
use crate::decode::payload::{decode_payload, payload_at};
use crate::decode::{filter, vector};
use crate::json::{self, child, index, invalid};
use qql_core::ast::{
    ClearPayloadStmt, DeletePayloadStmt, DeleteStmt, DeleteVectorStmt, PointEntry, PointSelector,
    PointVectors, ShardKey, UpdatePayloadStmt, UpdateVectorPoint, UpdateVectorStmt, UpsertPoint,
    UpsertStmt,
};

/// Decode a `PUT /points` (`PointInsertOperations`) body into `UPSERT`.
pub(crate) fn upsert(body: &Value, ctx: DecodeCtx<'_>) -> Result<UpsertStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &[
            "points",
            "batch",
            "shard_key",
            "update_filter",
            "update_mode",
        ],
    )?;
    let shard_key = vector::shard_key_field(obj, path)?;
    let update_filter = match obj.get("update_filter").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => filter::filter_opt(value, &child(path, "update_filter"))?,
    };
    let update_mode = match obj.get("update_mode").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(update_mode_value(value, &child(path, "update_mode"))?),
    };
    let points = if let Some(batch) = obj.get("batch") {
        decode_batch(batch, &child(path, "batch"))?
    } else {
        let points_path = child(path, "points");
        json::required(obj, "points", path)
            .and_then(|v| json::array(v, &points_path))?
            .iter()
            .enumerate()
            .map(|(i, point)| decode_point_struct(point, &index(&points_path, i)))
            .collect::<Result<Vec<_>, _>>()?
    };
    if points.is_empty() {
        return Err(invalid(
            child(path, "points"),
            "upsert requires at least one point",
        ));
    }
    Ok(UpsertStmt {
        collection: ctx.collection.to_string(),
        points,
        embedding: None,
        embed: Vec::new(),
        update_filter,
        update_mode,
        shard_key,
        wait: ctx.opts.wait,
    })
}

/// Decode an OpenAPI `UpdateMode` string into `UpsertUpdateMode`.
fn update_mode_value(
    value: &Value,
    path: &str,
) -> Result<qql_core::ast::UpsertUpdateMode, ConvertError> {
    use qql_core::ast::UpsertUpdateMode;
    match value.as_str() {
        Some("insert_only") => Ok(UpsertUpdateMode::InsertOnly),
        Some("update_only") => Ok(UpsertUpdateMode::UpdateOnly),
        Some("upsert") => Ok(UpsertUpdateMode::Upsert),
        Some(other) => Err(invalid(
            path,
            format!("unknown update_mode '{other}' (expected insert_only, update_only, or upsert)"),
        )),
        None => Err(invalid(
            path,
            format!(
                "expected update_mode string, got {}",
                json::type_name(value)
            ),
        )),
    }
}

/// Decode a `Batch` (`{ids, vectors, payloads?}`) into inline points.
fn decode_batch(value: &Value, path: &str) -> Result<Vec<PointEntry>, ConvertError> {
    let batch = json::object(value, path)?;
    let ids_path = child(path, "ids");
    let ids = json::required(batch, "ids", path)
        .and_then(|v| json::array(v, &ids_path))?
        .iter()
        .enumerate()
        .map(|(i, id)| vector::point_id(id, &index(&ids_path, i)))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.is_empty() {
        return Err(invalid(ids_path, "batch ids must not be empty"));
    }
    let payloads = match batch.get("payloads").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(value) => json::array(value, &child(path, "payloads"))?.clone(),
    };
    if !payloads.is_empty() && payloads.len() != ids.len() {
        return Err(invalid(
            child(path, "payloads"),
            format!(
                "payloads length {} does not match ids length {}",
                payloads.len(),
                ids.len()
            ),
        ));
    }
    let vectors = decode_batch_vectors(
        json::required(batch, "vectors", path)?,
        &child(path, "vectors"),
        ids.len(),
    )?;
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| {
            Ok(PointEntry::Inline(UpsertPoint {
                id,
                vectors: Some(vectors[i].clone()),
                payload: payload_at(&payloads, i, &child(path, "payloads"))?,
            }))
        })
        .collect()
}

/// Decode a `BatchVectorStruct` into one `PointVectors` per batch id.
fn decode_batch_vectors(
    value: &Value,
    path: &str,
    count: usize,
) -> Result<Vec<PointVectors>, ConvertError> {
    match value {
        Value::Array(items) => {
            if items.len() != count {
                return Err(invalid(
                    path,
                    format!(
                        "batch vectors length {} does not match ids length {count}",
                        items.len()
                    ),
                ));
            }
            items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    Ok(PointVectors::Unnamed(vector::vector_value(
                        item,
                        &index(path, i),
                    )?))
                })
                .collect()
        }
        Value::Object(named) => {
            let mut per_point: Vec<Vec<(String, qql_core::ast::VectorValue)>> =
                vec![Vec::new(); count];
            for (name, column) in named {
                let column_path = child(path, name);
                let values = json::array(column, &column_path)?;
                if values.len() != count {
                    return Err(invalid(
                        column_path,
                        format!(
                            "batch vector column length {} does not match ids length {count}",
                            values.len()
                        ),
                    ));
                }
                for (i, item) in values.iter().enumerate() {
                    per_point[i].push((
                        name.clone(),
                        vector::vector_value(item, &index(&column_path, i))?,
                    ));
                }
            }
            Ok(per_point.into_iter().map(PointVectors::Named).collect())
        }
        other => Err(invalid(
            path,
            format!(
                "expected batch vectors (per-point array or name map), got {}",
                json::type_name(other)
            ),
        )),
    }
}

/// Decode one `PointStruct` (`{id, vector?, payload?}`).
fn decode_point_struct(value: &Value, path: &str) -> Result<PointEntry, ConvertError> {
    let obj = json::object(value, path)?;
    let id = vector::point_id(json::required(obj, "id", path)?, &child(path, "id"))?;
    let vectors = match obj.get("vector").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(vector::point_vectors(value, &child(path, "vector"))?),
    };
    let payload = match obj.get("payload").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(value) => decode_payload(value, &child(path, "payload"))?,
    };
    Ok(PointEntry::Inline(UpsertPoint {
        id,
        vectors,
        payload,
    }))
}

/// Decode a `PointsSelector` / `{points, filter}` body into a selector.
fn selector(obj: &json::Obj, path: &str) -> Result<PointSelector, ConvertError> {
    let points = obj.get("points").filter(|v| !v.is_null());
    let filter = filter::filter_field(obj, "filter", path)?;
    match (points, filter) {
        (Some(value), None) => {
            let list_path = child(path, "points");
            let ids = json::array(value, &list_path)?
                .iter()
                .enumerate()
                .map(|(i, id)| vector::point_id(id, &index(&list_path, i)))
                .collect::<Result<Vec<_>, _>>()?;
            if ids.is_empty() {
                return Err(invalid(list_path, "selector must list at least one point"));
            }
            Ok(PointSelector::Ids(ids))
        }
        (None, Some(filter)) => Ok(PointSelector::Filter(filter)),
        (Some(_), Some(_)) => Err(invalid(
            path,
            "selector cannot combine `points` and `filter`",
        )),
        (None, None) => Err(invalid(path, "selector requires `points` or `filter`")),
    }
}

/// Decode a `POST /points/delete` body into `DELETE`.
pub(crate) fn delete(body: &Value, ctx: DecodeCtx<'_>) -> Result<DeleteStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["points", "filter", "shard_key"])?;
    Ok(DeleteStmt {
        collection: ctx.collection.to_string(),
        selector: selector(obj, path)?,
        shard_key: vector::shard_key_field(obj, path)?,
        wait: ctx.opts.wait,
    })
}

/// Decode a `POST /points/payload/clear` body into `CLEAR PAYLOAD`.
pub(crate) fn clear_payload(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<ClearPayloadStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["points", "filter", "shard_key"])?;
    Ok(ClearPayloadStmt {
        collection: ctx.collection.to_string(),
        selector: selector(obj, path)?,
        shard_key: vector::shard_key_field(obj, path)?,
        wait: ctx.opts.wait,
    })
}

/// Decode a `POST /points/payload` (`SetPayload`) body into `UPDATE … SET PAYLOAD`.
pub(crate) fn update_payload(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<UpdatePayloadStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &["payload", "points", "filter", "shard_key", "key"],
    )?;
    let key = match obj.get("key").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(json::string_at(value, &child(path, "key"))?.to_string()),
    };
    let payload = decode_payload(
        json::required(obj, "payload", path)?,
        &child(path, "payload"),
    )?;
    Ok(UpdatePayloadStmt {
        collection: ctx.collection.to_string(),
        selector: selector(obj, path)?,
        payload,
        key,
        overwrite: false,
        shard_key: vector::shard_key_field(obj, path)?,
        wait: ctx.opts.wait,
    })
}

/// Decode a `POST /points/payload/delete` body into `DELETE PAYLOAD`.
pub(crate) fn delete_payload(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<DeletePayloadStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["keys", "points", "filter", "shard_key"])?;
    let keys_path = child(path, "keys");
    let keys = json::required(obj, "keys", path)
        .and_then(|v| json::array(v, &keys_path))?
        .iter()
        .enumerate()
        .map(|(i, key)| Ok(json::string_at(key, &index(&keys_path, i))?.to_string()))
        .collect::<Result<Vec<String>, ConvertError>>()?;
    if keys.is_empty() {
        return Err(invalid(
            keys_path,
            "keys must list at least one payload key",
        ));
    }
    Ok(DeletePayloadStmt {
        collection: ctx.collection.to_string(),
        keys,
        selector: selector(obj, path)?,
        shard_key: vector::shard_key_field(obj, path)?,
        wait: ctx.opts.wait,
    })
}

/// Decode a `POST /points/vectors/delete` body into `DELETE VECTOR`.
pub(crate) fn delete_vectors(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<DeleteVectorStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["vector", "points", "filter", "shard_key"])?;
    let names_path = child(path, "vector");
    let vector_names = json::required(obj, "vector", path)
        .and_then(|v| json::array(v, &names_path))?
        .iter()
        .enumerate()
        .map(|(i, name)| Ok(json::string_at(name, &index(&names_path, i))?.to_string()))
        .collect::<Result<Vec<String>, ConvertError>>()?;
    if vector_names.is_empty() {
        return Err(invalid(
            names_path,
            "vector must list at least one vector name",
        ));
    }
    Ok(DeleteVectorStmt {
        collection: ctx.collection.to_string(),
        selector: selector(obj, path)?,
        vector_names,
        shard_key: vector::shard_key_field(obj, path)?,
        wait: ctx.opts.wait,
    })
}

/// Decode a `PUT /points/vectors` (`UpdateVectors`) body into one
/// `UPDATE … SET VECTOR` statement. Named maps and multiple points stay on
/// the statement; the formatter picks compact `WHERE id =` vs `VALUES`.
pub(crate) fn update_vectors(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<UpdateVectorStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["points", "shard_key", "update_filter"])?;
    if obj.contains_key("update_filter") {
        return Err(invalid(
            child(path, "update_filter"),
            "update_filter has no QQL representation",
        ));
    }
    let shard_key: Option<ShardKey> = vector::shard_key_field(obj, path)?;
    let points_path = child(path, "points");
    let points = json::required(obj, "points", path).and_then(|v| json::array(v, &points_path))?;
    if points.is_empty() {
        return Err(invalid(points_path, "points must not be empty"));
    }
    let mut decoded = Vec::with_capacity(points.len());
    for (i, point) in points.iter().enumerate() {
        let point_path = index(&points_path, i);
        let point = json::object(point, &point_path)?;
        decoded.push(UpdateVectorPoint {
            id: vector::point_id(
                json::required(point, "id", &point_path)?,
                &child(&point_path, "id"),
            )?,
            vectors: vector::point_vectors(
                json::required(point, "vector", &point_path)?,
                &child(&point_path, "vector"),
            )?,
        });
    }
    Ok(UpdateVectorStmt {
        collection: ctx.collection.to_string(),
        points: decoded,
        shard_key,
        wait: ctx.opts.wait,
    })
}
