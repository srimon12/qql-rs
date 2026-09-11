//! Bare-body operation detection (no `{method, path}` context).
//!
//! Bare input is inherently heuristic: several Qdrant request schemas share a
//! shape. Detection is a fixed, documented order; the chosen operation is
//! decoded by the same strict decoders the wrapped path uses. Shapes that hit
//! an ambiguity with no default (a lone `filter`, a lone `shard_key`, an
//! empty `points` array) fail with [`ConvertError::UndecodableBody`] rather
//! than guessing.

use serde_json::{Map, Value};

use crate::ConvertError;
use crate::decode::{ddl, index, mutation, points, query};
use qql_core::ast::Stmt;

/// Detect and decode a bare body using `collection` as the target name.
pub(crate) fn convert(raw: &Value, collection: &str) -> Result<Vec<Stmt>, ConvertError> {
    let Some(obj) = raw.as_object() else {
        return Err(ConvertError::undecodable(format!(
            "expected a JSON object, got {}",
            crate::json::type_name(raw)
        )));
    };

    // 1. Grouped queries (group_by discriminates from plain QueryRequest).
    if obj.contains_key("query") && obj.contains_key("group_by") {
        return Ok(vec![Stmt::Query(Box::new(query::query_groups_request(
            raw, collection,
        )?))]);
    }
    // 2. Point batch upsert.
    if obj.contains_key("batch") {
        return Ok(vec![Stmt::Upsert(Box::new(mutation::upsert(
            raw, collection,
        )?))]);
    }
    // 3. SetPayload (`payload` + selector).
    if obj.contains_key("payload") && (obj.contains_key("points") || obj.contains_key("filter")) {
        return Ok(vec![Stmt::UpdatePayload(Box::new(
            mutation::update_payload(raw, collection)?,
        ))]);
    }
    // 4. DeletePayload (`keys` + selector).
    if obj.contains_key("keys") {
        return Ok(vec![Stmt::DeletePayload(Box::new(
            mutation::delete_payload(raw, collection)?,
        ))]);
    }
    // 5. DeleteVectors (`vector` + selector).
    if obj.contains_key("vector") && (obj.contains_key("points") || obj.contains_key("filter")) {
        return Ok(vec![Stmt::DeleteVector(Box::new(
            mutation::delete_vectors(raw, collection)?,
        ))]);
    }
    // 6. Points array: point objects upsert, scalar ids delete.
    if let Some(Value::Array(items)) = obj.get("points") {
        if items.is_empty() {
            return Err(ConvertError::undecodable(
                "empty `points` array matches several operations (upsert / delete / payload writes)",
            ));
        }
        if items.first().is_some_and(Value::is_object) {
            return Ok(vec![Stmt::Upsert(Box::new(mutation::upsert(
                raw, collection,
            )?))]);
        }
        return Ok(vec![Stmt::Delete(Box::new(mutation::delete(
            raw, collection,
        )?))]);
    }
    // 7. Point lookup by ids.
    if obj.contains_key("ids") {
        return Ok(vec![Stmt::Query(Box::new(points::point_request(
            raw, collection,
        )?))]);
    }
    // 8. Fusion / sample / order_by / formula query bodies (before the generic
    //    query/prefetch rule so a fusion-with-prefetch body synthesizes its
    //    `query` member instead of failing on a missing one).
    for key in ["fusion", "sample", "order_by", "formula"] {
        if let Some(value) = obj.get(key) {
            let synthesized = synthetic_query(obj, serde_json::json!({ key: value }));
            return query_stmt(synthesized, collection, false);
        }
    }
    // 9. QueryRequest / QueryGroupsRequest by `query` / `prefetch`.
    if obj.contains_key("query") || obj.contains_key("prefetch") {
        return Ok(vec![Stmt::Query(Box::new(query::query_request(
            raw, collection,
        )?))]);
    }
    // 10. Legacy search body: top-level `vector` becomes `{"nearest": …}`.
    if let Some(vector) = obj.get("vector") {
        let synthesized = synthetic_query(obj, serde_json::json!({ "nearest": vector }));
        return query_stmt(synthesized, collection, obj.contains_key("group_by"));
    }
    // 11. Legacy recommend body.
    if obj.contains_key("positive") || obj.contains_key("negative") {
        let recommend = Map::from_iter([
            (
                "positive".to_string(),
                obj.get("positive").cloned().unwrap_or(Value::Null),
            ),
            (
                "negative".to_string(),
                obj.get("negative").cloned().unwrap_or(Value::Null),
            ),
            (
                "strategy".to_string(),
                obj.get("strategy").cloned().unwrap_or(Value::Null),
            ),
        ]);
        let synthesized = synthetic_query(
            obj,
            serde_json::json!({ "recommend": Value::Object(recommend) }),
        );
        return query_stmt(synthesized, collection, false);
    }
    // 12. Legacy discover body.
    if obj.contains_key("target") && obj.contains_key("context") {
        let discover = Map::from_iter([
            (
                "target".to_string(),
                obj.get("target").cloned().unwrap_or(Value::Null),
            ),
            (
                "context".to_string(),
                obj.get("context").cloned().unwrap_or(Value::Null),
            ),
        ]);
        let synthesized = synthetic_query(
            obj,
            serde_json::json!({ "discover": Value::Object(discover) }),
        );
        return query_stmt(synthesized, collection, false);
    }
    // 13. Context-pair query body.
    if let Some(context) = obj.get("context") {
        let synthesized = synthetic_query(obj, serde_json::json!({ "context": context }));
        return query_stmt(synthesized, collection, false);
    }
    // 14. Collection create body.
    if obj.contains_key("vectors") {
        return Ok(vec![Stmt::CreateCollection(Box::new(
            ddl::create_collection(raw, collection)?,
        ))]);
    }
    // 15. Index create body.
    if obj.contains_key("field_name") {
        return Ok(vec![Stmt::CreateIndex(Box::new(index::create_index(
            raw, collection,
        )?))]);
    }
    // 16. Shard-key creation (a bare `shard_key` alone is ambiguous with drop).
    if obj.contains_key("shard_key") {
        if obj.contains_key("shards_number") || obj.contains_key("replication_factor") {
            return Ok(vec![Stmt::CreateShardKey(Box::new(ddl::create_shard_key(
                raw, collection,
            )?))]);
        }
        return Err(ConvertError::undecodable(
            "a lone `shard_key` is ambiguous between CREATE SHARD KEY and DROP SHARD KEY; wrap the request with method/path",
        ));
    }
    // 17. Quota replacement body.
    if [
        "enabled",
        "max_resident_memory_percent",
        "max_disk_usage_percent",
        "release_margin_percent",
    ]
    .iter()
    .any(|key| obj.contains_key(*key))
    {
        return Ok(vec![Stmt::SetQuota(Box::new(ddl::set_quota(raw)?))]);
    }
    // 18. Facet aggregation.
    if obj.contains_key("key") {
        return Ok(vec![Stmt::Facet(Box::new(points::facet(raw, collection)?))]);
    }
    // 19. Filter-first bodies: scroll with limit, count with exact, else delete.
    if obj.contains_key("filter") {
        if obj.contains_key("limit") {
            return Ok(vec![Stmt::Scroll(Box::new(points::scroll(
                raw, collection,
            )?))]);
        }
        if obj.contains_key("exact") {
            return Ok(vec![Stmt::Count(Box::new(points::count(raw, collection)?))]);
        }
        return Ok(vec![Stmt::Delete(Box::new(mutation::delete(
            raw, collection,
        )?))]);
    }
    Err(ConvertError::undecodable(
        "cannot detect an operation from this JSON structure; wrap the request with method/path",
    ))
}

/// Clone a bare body with `query` set to `query`.
fn synthetic_query(obj: &Map<String, Value>, query: Value) -> Value {
    let mut synthetic = obj.clone();
    synthetic.insert("query".to_string(), query);
    Value::Object(synthetic)
}

/// Decode a synthesized query body, choosing the groups endpoint when needed.
fn query_stmt(
    synthesized: Value,
    collection: &str,
    grouped: bool,
) -> Result<Vec<Stmt>, ConvertError> {
    let request = if grouped {
        query::query_groups_request(&synthesized, collection)?
    } else {
        query::query_request(&synthesized, collection)?
    };
    Ok(vec![Stmt::Query(Box::new(request))])
}
