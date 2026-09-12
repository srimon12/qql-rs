//! Decode point retrieval / scroll / count / facet request bodies.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::DecodeCtx;
use crate::decode::params::{decode_with_payload, decode_with_vector};
use crate::decode::{filter, vector};
use crate::json::{self, child, index, invalid};
use qql_core::ast::{
    CountStmt, FacetStmt, PageSpec, PointId, QueryCollection, QueryExpr, QueryOutput, QueryStmt,
    ScrollStmt,
};

/// Decode a `POST /points` (`PointRequest`) body into `QUERY POINTS`.
pub(crate) fn point_request(body: &Value, ctx: DecodeCtx<'_>) -> Result<QueryStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &["ids", "with_payload", "with_vector", "shard_key"],
    )?;
    let ids_path = child(path, "ids");
    let ids = json::required(obj, "ids", path)
        .and_then(|v| json::array(v, &ids_path))?
        .iter()
        .enumerate()
        .map(|(i, item)| vector::point_id(item, &index(&ids_path, i)))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.is_empty() {
        return Err(invalid(ids_path, "ids must list at least one point"));
    }
    Ok(QueryStmt {
        ctes: Vec::new(),
        collection: QueryCollection::Explicit(ctx.collection.to_string()),
        expression: QueryExpr::Points { ids },
        filter: None,
        params: None,
        score_threshold: None,
        group: None,
        output: QueryOutput {
            payload: decode_with_payload(obj, path)?,
            vectors: decode_with_vector(obj, path)?,
        },
        page: PageSpec::default(),
        shard_key: vector::shard_key_field(obj, path)?,
    })
}

/// Decode a `POST /points/scroll` (`ScrollRequest`) body into `SCROLL`.
pub(crate) fn scroll(body: &Value, ctx: DecodeCtx<'_>) -> Result<ScrollStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &[
            "offset",
            "limit",
            "filter",
            "with_payload",
            "with_vector",
            "order_by",
            "shard_key",
        ],
    )?;
    if obj.contains_key("order_by") {
        return Err(invalid(
            child(path, "order_by"),
            "SCROLL order_by has no QQL representation; use QUERY ORDER BY",
        ));
    }
    if let Some(payload) = obj.get("with_payload").filter(|v| !v.is_null())
        && payload != &Value::Bool(true)
    {
        return Err(invalid(
            child(path, "with_payload"),
            "SCROLL always returns payload in QQL; with_payload false / field lists have no representation",
        ));
    }
    let limit = json::opt_u64(obj, "limit", path)?.unwrap_or(10);
    let after = match obj.get("offset").filter(|v| !v.is_null()) {
        None => None,
        Some(offset) => offset_to_after(&vector::point_id(offset, &child(path, "offset"))?),
    };
    Ok(ScrollStmt {
        collection: ctx.collection.to_string(),
        limit,
        filter: filter::filter_field(obj, "filter", path)?,
        after,
        shard_key: vector::shard_key_field(obj, path)?,
        with_vector: decode_with_vector(obj, path)?,
        limit_param: None,
        limit_span: None,
    })
}

/// Invert the planner's exclusive `AFTER` cursor: wire `offset = after + 1`.
fn offset_to_after(offset: &PointId) -> Option<PointId> {
    match offset {
        PointId::Number(0) => None,
        PointId::Number(n) => Some(PointId::Number(n - 1)),
        PointId::String(value) => Some(PointId::String(decrement_uuid(value))),
        PointId::Param(..) | PointId::PositionalParam(..) => Some(offset.clone()),
    }
}

/// Decrement a UUID-like string by one; non-UUID strings pass through.
///
/// `qql_plan::mutation::increment_uuid_point_id` only rewrites 32-hex UUIDs
/// and leaves everything else untouched, so this mirrors that rule exactly.
fn decrement_uuid(value: &str) -> String {
    let clean: String = value.chars().filter(|c| *c != '-').collect();
    if clean.len() != 32 {
        return value.to_string();
    }
    let Some(current) = u128::from_str_radix(&clean, 16).ok() else {
        return value.to_string();
    };
    let Some(previous) = current.checked_sub(1) else {
        return value.to_string();
    };
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (previous >> 96) as u32,
        ((previous >> 80) & 0xffff) as u16,
        ((previous >> 64) & 0xffff) as u16,
        ((previous >> 48) & 0xffff) as u16,
        (previous & 0xffff_ffff_ffff) as u64
    )
}

/// Decode a `POST /points/count` (`CountRequest`) body into `COUNT`.
pub(crate) fn count(body: &Value, ctx: DecodeCtx<'_>) -> Result<CountStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["filter", "exact", "shard_key"])?;
    Ok(CountStmt {
        collection: QueryCollection::Explicit(ctx.collection.to_string()),
        filter: filter::filter_field(obj, "filter", path)?,
        shard_key: vector::shard_key_field(obj, path)?,
        exact: json::opt_bool(obj, "exact", path)?,
    })
}

/// Decode a `POST /facet` (`FacetRequest`) body into `FACET`.
pub(crate) fn facet(body: &Value, ctx: DecodeCtx<'_>) -> Result<FacetStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["key", "filter", "limit", "exact", "shard_key"])?;
    Ok(FacetStmt {
        key: json::required_str(obj, "key", path)?,
        collection: QueryCollection::Explicit(ctx.collection.to_string()),
        filter: filter::filter_field(obj, "filter", path)?,
        limit: json::opt_u64(obj, "limit", path)?,
        exact: json::opt_bool(obj, "exact", path)?,
        shard_key: vector::shard_key_field(obj, path)?,
        limit_param: None,
        limit_span: None,
    })
}
