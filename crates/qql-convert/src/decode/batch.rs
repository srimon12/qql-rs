//! Batch endpoints: one wrapped request decoding to one `BATCH` block.
//!
//! `POST …/points/query/batch` carries `{searches: [QueryRequest, …]}` and
//! `POST …/points/batch` carries `{operations: [UpdateOperation, …]}`. Each
//! element decodes through the same strict decoder as its single-request
//! endpoint, and the shared query params become the block header (`WAIT` /
//! `PARAMS`), so captures replay as one batch RPC.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::DecodeCtx;
use crate::decode::{mutation, query};
use crate::json::{self, child, index, invalid};
use qql_core::ast::{BatchStmt, Stmt};

/// Decode a `POST …/points/query/batch` body into one query `BATCH` block.
pub(crate) fn query_batch(body: &Value, ctx: DecodeCtx<'_>) -> Result<Vec<Stmt>, ConvertError> {
    ctx.opts.reject_wait()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["searches"])?;
    let searches = json::array(
        json::required(obj, "searches", path)?,
        &child(path, "searches"),
    )?;
    if searches.is_empty() {
        return Err(invalid(
            child(path, "searches"),
            "query batch requires at least one search",
        ));
    }
    // Members decode with scrubbed opts: shared `?timeout=` / `?consistency=`
    // live on the block header, and the planner rejects members repeating them.
    let default_opts = crate::request::RequestOpts::default();
    let member_ctx = DecodeCtx {
        collection: ctx.collection,
        opts: &default_opts,
    };
    // Entry errors carry the inner `body.*` field path: the shared query
    // decoder addresses fields from the request root. The batch index is
    // recoverable from statement order (one member per search, in order).
    let mut statements = Vec::with_capacity(searches.len());
    for search in searches {
        let member = query::query_request(search, member_ctx)?;
        statements.push(Stmt::Query(Box::new(member)));
    }
    // Shared read opts become the block header; members must not repeat them.
    let params = shared_read_params(ctx);
    Ok(vec![Stmt::Batch(Box::new(BatchStmt {
        statements,
        wait: None,
        params,
    }))])
}

/// Shared `?timeout=` / `?consistency=` as block-header `PARAMS`, if any.
fn shared_read_params(ctx: DecodeCtx<'_>) -> Option<qql_core::ast::SearchParams> {
    if ctx.opts.timeout.is_none() && ctx.opts.consistency.is_none() {
        return None;
    }
    let params = qql_core::ast::SearchParams {
        timeout: ctx.opts.timeout,
        consistency: ctx.opts.consistency.clone(),
        ..Default::default()
    };
    Some(params)
}

/// Decode a `POST …/points/batch` body into one mutation `BATCH` block.
///
/// Each `{op: inner}` wrapper unwraps to the inner schema shared with the
/// single-request endpoint, decoded by the same function.
pub(crate) fn points_batch(body: &Value, ctx: DecodeCtx<'_>) -> Result<Vec<Stmt>, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["operations"])?;
    let operations = json::array(
        json::required(obj, "operations", path)?,
        &child(path, "operations"),
    )?;
    if operations.is_empty() {
        return Err(invalid(
            child(path, "operations"),
            "points batch requires at least one operation",
        ));
    }
    let mut statements = Vec::with_capacity(operations.len());
    // Members decode with scrubbed opts: shared `?wait=` lives on the
    // block header, and the planner rejects members repeating it.
    let default_opts = crate::request::RequestOpts::default();
    let member_ctx = DecodeCtx {
        collection: ctx.collection,
        opts: &default_opts,
    };
    for (i, operation) in operations.iter().enumerate() {
        let entry = index(&child(path, "operations"), i);
        statements.push(batch_operation(operation, &entry, member_ctx)?);
    }
    Ok(vec![Stmt::Batch(Box::new(BatchStmt {
        statements,
        wait: ctx.opts.wait,
        params: None,
    }))])
}

/// Decode one `{op: inner}` batch operation.
fn batch_operation(
    operation: &Value,
    path: &str,
    ctx: DecodeCtx<'_>,
) -> Result<Stmt, ConvertError> {
    let obj = json::object(operation, path)?;
    let mut keys = obj.keys();
    let (Some(key), None) = (keys.next(), keys.next()) else {
        return Err(invalid(
            path,
            "batch operation must contain exactly one operation",
        ));
    };
    let inner = json::required(obj, key, path)?;
    Ok(match key.as_str() {
        "upsert" => Stmt::Upsert(Box::new(mutation::upsert(inner, ctx)?)),
        "delete" => Stmt::Delete(Box::new(mutation::delete(inner, ctx)?)),
        "set_payload" => Stmt::UpdatePayload(Box::new(mutation::update_payload(inner, ctx)?)),
        "overwrite_payload" => {
            let mut stmt = mutation::update_payload(inner, ctx)?;
            stmt.overwrite = true;
            Stmt::UpdatePayload(Box::new(stmt))
        }
        "delete_payload" => Stmt::DeletePayload(Box::new(mutation::delete_payload(inner, ctx)?)),
        "clear_payload" => Stmt::ClearPayload(Box::new(mutation::clear_payload(inner, ctx)?)),
        "update_vectors" => Stmt::UpdateVector(Box::new(mutation::update_vectors(inner, ctx)?)),
        "delete_vectors" => Stmt::DeleteVector(Box::new(mutation::delete_vectors(inner, ctx)?)),
        other => {
            return Err(invalid(
                child(path, other),
                format!("unknown batch operation '{other}'"),
            ));
        }
    })
}
