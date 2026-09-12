//! Endpoint dispatch: matrix operation + body → typed statements.

pub(crate) mod batch;
pub(crate) mod condition;
pub(crate) mod config;
pub(crate) mod ddl;
pub(crate) mod filter;
pub(crate) mod formula;
pub(crate) mod index;
pub(crate) mod interface;
pub(crate) mod modes;
pub(crate) mod mutation;
pub(crate) mod params;
pub(crate) mod payload;
pub(crate) mod points;
pub(crate) mod predicates;
pub(crate) mod prefetch;
pub(crate) mod quantization;
pub(crate) mod query;
pub(crate) mod vector;

use serde_json::Value;

use crate::ConvertError;
use crate::endpoint::{Endpoint, EndpointMatch};
use crate::request::RequestOpts;
use qql_core::ast::Stmt;

/// Per-request decode context: collection name plus recovered query opts.
#[derive(Clone, Copy)]
pub(crate) struct DecodeCtx<'a> {
    /// Path-derived or caller-supplied collection.
    pub collection: &'a str,
    /// `wait` / `timeout` / `consistency` from the wrapped query string.
    pub opts: &'a RequestOpts,
}

/// Decode one wrapped request into typed statements (pre-formatting).
pub(crate) fn endpoint(
    matched: &EndpointMatch,
    body: Option<&Value>,
    ctx: DecodeCtx<'_>,
) -> Result<Vec<Stmt>, ConvertError> {
    let body = match body {
        Some(body) => body,
        None if matched.op.body_required() => {
            return Err(ConvertError::undecodable(format!(
                "{} requires a request body",
                endpoint_name(matched.op)
            )));
        }
        None => &Value::Null,
    };
    Ok(match matched.op {
        Endpoint::Query => vec![Stmt::Query(Box::new(query::query_request(body, ctx)?))],
        Endpoint::QueryGroups => {
            vec![Stmt::Query(Box::new(query::query_groups_request(
                body, ctx,
            )?))]
        }
        Endpoint::GetPoints => vec![Stmt::Query(Box::new(points::point_request(body, ctx)?))],
        Endpoint::Facet => vec![Stmt::Facet(Box::new(points::facet(body, ctx)?))],
        Endpoint::Scroll => vec![Stmt::Scroll(Box::new(points::scroll(body, ctx)?))],
        Endpoint::Count => vec![Stmt::Count(Box::new(points::count(body, ctx)?))],
        Endpoint::Upsert => vec![Stmt::Upsert(Box::new(mutation::upsert(body, ctx)?))],
        Endpoint::Delete => vec![Stmt::Delete(Box::new(mutation::delete(body, ctx)?))],
        Endpoint::ClearPayload => vec![Stmt::ClearPayload(Box::new(mutation::clear_payload(
            body, ctx,
        )?))],
        Endpoint::DeletePayload => vec![Stmt::DeletePayload(Box::new(mutation::delete_payload(
            body, ctx,
        )?))],
        Endpoint::DeleteVectors => vec![Stmt::DeleteVector(Box::new(mutation::delete_vectors(
            body, ctx,
        )?))],
        Endpoint::UpdateVectors => vec![Stmt::UpdateVector(Box::new(mutation::update_vectors(
            body, ctx,
        )?))],
        Endpoint::UpdatePayload => vec![Stmt::UpdatePayload(Box::new(mutation::update_payload(
            body, ctx,
        )?))],
        Endpoint::CreateCollection => vec![Stmt::CreateCollection(Box::new(
            ddl::create_collection(body, ctx)?,
        ))],
        Endpoint::AlterCollection => vec![Stmt::AlterCollection(Box::new(ddl::alter_collection(
            body, ctx,
        )?))],
        Endpoint::DropCollection => {
            reject_body(body)?;
            ctx.opts.reject_wait()?;
            ctx.opts.reject_read()?;
            vec![Stmt::DropCollection(Box::new(
                qql_core::ast::DropCollectionStmt {
                    collection: ctx.collection.to_string(),
                },
            ))]
        }
        Endpoint::CreateIndex => vec![Stmt::CreateIndex(Box::new(index::create_index(body, ctx)?))],
        Endpoint::DropIndex => {
            reject_body(body)?;
            ctx.opts.reject_wait()?;
            ctx.opts.reject_read()?;
            let field = matched.field.clone().ok_or_else(|| {
                ConvertError::undecodable("DROP INDEX requires a field path segment")
            })?;
            vec![Stmt::DropIndex(Box::new(qql_core::ast::DropIndexStmt {
                collection: ctx.collection.to_string(),
                field,
            }))]
        }
        Endpoint::CreateShardKey => vec![Stmt::CreateShardKey(Box::new(ddl::create_shard_key(
            body, ctx,
        )?))],
        Endpoint::DropShardKey => {
            vec![Stmt::DropShardKey(Box::new(ddl::drop_shard_key(
                body, ctx,
            )?))]
        }
        Endpoint::ShowShardKeys => {
            reject_body(body)?;
            ctx.opts.reject_wait()?;
            ctx.opts.reject_read()?;
            vec![Stmt::ShowShardKeys(ctx.collection.to_string())]
        }
        Endpoint::ShowCollections => {
            reject_body(body)?;
            ctx.opts.reject_wait()?;
            ctx.opts.reject_read()?;
            vec![Stmt::ShowCollections]
        }
        Endpoint::ShowCollection => {
            reject_body(body)?;
            ctx.opts.reject_wait()?;
            ctx.opts.reject_read()?;
            vec![Stmt::ShowCollection(ctx.collection.to_string())]
        }
        Endpoint::ShowQuotas => {
            reject_body(body)?;
            ctx.opts.reject_wait()?;
            ctx.opts.reject_read()?;
            vec![Stmt::ShowQuotas]
        }
        Endpoint::SetQuota => vec![Stmt::SetQuota(Box::new(ddl::set_quota(body, ctx)?))],
        Endpoint::QueryBatch => batch::query_batch(body, ctx)?,
        Endpoint::PointsBatch => batch::points_batch(body, ctx)?,
    })
}

/// Bodyless routes carry no JSON body. `None`, `null`, and `{}` are accepted
/// for tolerance with recorders and tests. Any other body fails closed so a
/// wrapped `DELETE` with a payload cannot silently drop data.
fn reject_body(body: &Value) -> Result<(), ConvertError> {
    match body {
        Value::Null => Ok(()),
        Value::Object(obj) if obj.is_empty() => Ok(()),
        other => Err(crate::json::invalid(
            "body",
            format!(
                "bodyless endpoint must not carry a body, got {}",
                crate::json::type_name(other)
            ),
        )),
    }
}

/// Human-readable operation name for diagnostics.
fn endpoint_name(op: Endpoint) -> &'static str {
    match op {
        Endpoint::Query => "POST /points/query",
        Endpoint::QueryGroups => "POST /points/query/groups",
        Endpoint::GetPoints => "POST /points",
        Endpoint::Facet => "POST /facet",
        Endpoint::Scroll => "POST /points/scroll",
        Endpoint::Count => "POST /points/count",
        Endpoint::Upsert => "PUT /points",
        Endpoint::Delete => "POST /points/delete",
        Endpoint::ClearPayload => "POST /points/payload/clear",
        Endpoint::DeletePayload => "POST /points/payload/delete",
        Endpoint::DeleteVectors => "POST /points/vectors/delete",
        Endpoint::UpdateVectors => "PUT /points/vectors",
        Endpoint::UpdatePayload => "POST /points/payload",
        Endpoint::CreateCollection => "PUT /collections/{c}",
        Endpoint::AlterCollection => "PATCH /collections/{c}",
        Endpoint::DropCollection => "DELETE /collections/{c}",
        Endpoint::CreateIndex => "PUT /collections/{c}/index",
        Endpoint::DropIndex => "DELETE /collections/{c}/index/{field}",
        Endpoint::CreateShardKey => "PUT /collections/{c}/shards",
        Endpoint::DropShardKey => "POST /collections/{c}/shards/delete",
        Endpoint::ShowShardKeys => "GET /collections/{c}/shards",
        Endpoint::ShowCollections => "GET /collections",
        Endpoint::ShowCollection => "GET /collections/{c}",
        Endpoint::ShowQuotas => "GET /quotas",
        Endpoint::SetQuota => "PUT /quotas",
        Endpoint::QueryBatch => "POST /points/query/batch",
        Endpoint::PointsBatch => "POST /points/batch",
    }
}
