//! Decode point mutation request bodies (upsert, delete, payload/vector writes).

use serde_json::Value;

use crate::ConvertError;
use crate::decode::payload::{decode_payload, payload_at};
use crate::decode::query::decode_shard_key_field;
use crate::decode::{filter, vector};
use crate::json::{self, child, index, invalid};
use qql_core::ast::{
    ClearPayloadStmt, DeletePayloadStmt, DeleteStmt, DeleteVectorStmt, PointEntry, PointSelector,
    PointVectors, ShardKey, UpdatePayloadStmt, UpdateVectorStmt, UpsertPoint, UpsertStmt,
};

/// Decode a `PUT /points` (`PointInsertOperations`) body into `UPSERT`.
pub(crate) fn upsert(body: &Value, collection: &str) -> Result<UpsertStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    reject_update_guards(obj, path)?;
    let shard_key = decode_shard_key_field(obj, path)?;
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
        collection: collection.to_string(),
        points,
        embedding: None,
        embed: Vec::new(),
        shard_key,
        wait: None,
    })
}

/// Reject `update_filter` / `update_mode`, which have no QQL representation.
fn reject_update_guards(obj: &json::Obj, path: &str) -> Result<(), ConvertError> {
    for key in ["update_filter", "update_mode"] {
        if obj.contains_key(key) {
            return Err(invalid(
                child(path, key),
                format!("{key} has no QQL representation"),
            ));
        }
    }
    Ok(())
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
pub(crate) fn delete(body: &Value, collection: &str) -> Result<DeleteStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    Ok(DeleteStmt {
        collection: collection.to_string(),
        selector: selector(obj, path)?,
        shard_key: decode_shard_key_field(obj, path)?,
        wait: None,
    })
}

/// Decode a `POST /points/payload/clear` body into `CLEAR PAYLOAD`.
pub(crate) fn clear_payload(
    body: &Value,
    collection: &str,
) -> Result<ClearPayloadStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    Ok(ClearPayloadStmt {
        collection: collection.to_string(),
        selector: selector(obj, path)?,
        shard_key: decode_shard_key_field(obj, path)?,
        wait: None,
    })
}

/// Decode a `POST /points/payload` (`SetPayload`) body into `UPDATE … SET PAYLOAD`.
pub(crate) fn update_payload(
    body: &Value,
    collection: &str,
) -> Result<UpdatePayloadStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    if obj.contains_key("key") {
        return Err(invalid(
            child(path, "key"),
            "path-scoped SetPayload (`key`) has no QQL representation",
        ));
    }
    let payload = decode_payload(
        json::required(obj, "payload", path)?,
        &child(path, "payload"),
    )?;
    Ok(UpdatePayloadStmt {
        collection: collection.to_string(),
        selector: selector(obj, path)?,
        payload,
        shard_key: decode_shard_key_field(obj, path)?,
        wait: None,
    })
}

/// Decode a `POST /points/payload/delete` body into `DELETE PAYLOAD`.
pub(crate) fn delete_payload(
    body: &Value,
    collection: &str,
) -> Result<DeletePayloadStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
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
        collection: collection.to_string(),
        keys,
        selector: selector(obj, path)?,
        shard_key: decode_shard_key_field(obj, path)?,
        wait: None,
    })
}

/// Decode a `POST /points/vectors/delete` body into `DELETE VECTOR`.
pub(crate) fn delete_vectors(
    body: &Value,
    collection: &str,
) -> Result<DeleteVectorStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
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
        collection: collection.to_string(),
        selector: selector(obj, path)?,
        vector_names,
        shard_key: decode_shard_key_field(obj, path)?,
        wait: None,
    })
}

/// Decode a `PUT /points/vectors` (`UpdateVectors`) body into one
/// `UPDATE … SET VECTOR` statement per point/vector pair.
pub(crate) fn update_vectors(
    body: &Value,
    collection: &str,
) -> Result<Vec<UpdateVectorStmt>, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    if obj.contains_key("update_filter") {
        return Err(invalid(
            child(path, "update_filter"),
            "update_filter has no QQL representation",
        ));
    }
    let shard_key: Option<ShardKey> = decode_shard_key_field(obj, path)?;
    let points_path = child(path, "points");
    let points = json::required(obj, "points", path).and_then(|v| json::array(v, &points_path))?;
    if points.is_empty() {
        return Err(invalid(points_path, "points must not be empty"));
    }
    let mut statements = Vec::new();
    for (i, point) in points.iter().enumerate() {
        let point_path = index(&points_path, i);
        let point = json::object(point, &point_path)?;
        let id = vector::point_id(
            json::required(point, "id", &point_path)?,
            &child(&point_path, "id"),
        )?;
        let vectors = vector::point_vectors(
            json::required(point, "vector", &point_path)?,
            &child(&point_path, "vector"),
        )?;
        match vectors {
            PointVectors::Unnamed(value) => statements.push(UpdateVectorStmt {
                collection: collection.to_string(),
                point_id: id,
                vector: value,
                vector_name: None,
                shard_key: shard_key.clone(),
                wait: None,
            }),
            PointVectors::Named(entries) => {
                for (name, value) in entries {
                    statements.push(UpdateVectorStmt {
                        collection: collection.to_string(),
                        point_id: id.clone(),
                        vector: value,
                        vector_name: Some(name),
                        shard_key: shard_key.clone(),
                        wait: None,
                    });
                }
            }
            PointVectors::Param(..) | PointVectors::PositionalParam(..) => {
                return Err(invalid(
                    child(&point_path, "vector"),
                    "parameter placeholders cannot be decoded from JSON",
                ));
            }
        }
    }
    Ok(statements)
}
