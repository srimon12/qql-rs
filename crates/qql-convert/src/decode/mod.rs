//! Endpoint dispatch: matrix operation + body → typed statements.

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
use qql_core::ast::Stmt;

/// Decode one wrapped request into typed statements (pre-formatting).
pub(crate) fn endpoint(
    matched: &EndpointMatch,
    body: Option<&Value>,
    caller_collection: &str,
) -> Result<Vec<Stmt>, ConvertError> {
    let collection = matched
        .collection
        .clone()
        .unwrap_or_else(|| caller_collection.to_string());
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
        Endpoint::Query => vec![Stmt::Query(Box::new(query::query_request(
            body,
            &collection,
        )?))],
        Endpoint::QueryGroups => vec![Stmt::Query(Box::new(query::query_groups_request(
            body,
            &collection,
        )?))],
        Endpoint::GetPoints => vec![Stmt::Query(Box::new(points::point_request(
            body,
            &collection,
        )?))],
        Endpoint::Facet => vec![Stmt::Facet(Box::new(points::facet(body, &collection)?))],
        Endpoint::Scroll => vec![Stmt::Scroll(Box::new(points::scroll(body, &collection)?))],
        Endpoint::Count => vec![Stmt::Count(Box::new(points::count(body, &collection)?))],
        Endpoint::Upsert => vec![Stmt::Upsert(Box::new(mutation::upsert(body, &collection)?))],
        Endpoint::Delete => vec![Stmt::Delete(Box::new(mutation::delete(body, &collection)?))],
        Endpoint::ClearPayload => vec![Stmt::ClearPayload(Box::new(mutation::clear_payload(
            body,
            &collection,
        )?))],
        Endpoint::DeletePayload => vec![Stmt::DeletePayload(Box::new(mutation::delete_payload(
            body,
            &collection,
        )?))],
        Endpoint::DeleteVectors => vec![Stmt::DeleteVector(Box::new(mutation::delete_vectors(
            body,
            &collection,
        )?))],
        Endpoint::UpdateVectors => mutation::update_vectors(body, &collection)?
            .into_iter()
            .map(|stmt| Stmt::UpdateVector(Box::new(stmt)))
            .collect(),
        Endpoint::UpdatePayload => vec![Stmt::UpdatePayload(Box::new(mutation::update_payload(
            body,
            &collection,
        )?))],
        Endpoint::CreateCollection => vec![Stmt::CreateCollection(Box::new(
            ddl::create_collection(body, &collection)?,
        ))],
        Endpoint::AlterCollection => vec![Stmt::AlterCollection(Box::new(ddl::alter_collection(
            body,
            &collection,
        )?))],
        Endpoint::DropCollection => vec![Stmt::DropCollection(Box::new(
            qql_core::ast::DropCollectionStmt {
                collection: collection.clone(),
            },
        ))],
        Endpoint::CreateIndex => vec![Stmt::CreateIndex(Box::new(index::create_index(
            body,
            &collection,
        )?))],
        Endpoint::DropIndex => {
            let field = matched.field.clone().ok_or_else(|| {
                ConvertError::undecodable("DROP INDEX requires a field path segment")
            })?;
            vec![Stmt::DropIndex(Box::new(qql_core::ast::DropIndexStmt {
                collection: collection.clone(),
                field,
            }))]
        }
        Endpoint::CreateShardKey => vec![Stmt::CreateShardKey(Box::new(ddl::create_shard_key(
            body,
            &collection,
        )?))],
        Endpoint::DropShardKey => vec![Stmt::DropShardKey(Box::new(ddl::drop_shard_key(
            body,
            &collection,
        )?))],
        Endpoint::ShowShardKeys => vec![Stmt::ShowShardKeys(collection.clone())],
        Endpoint::ShowCollections => vec![Stmt::ShowCollections],
        Endpoint::ShowCollection => vec![Stmt::ShowCollection(collection.clone())],
        Endpoint::ShowQuotas => vec![Stmt::ShowQuotas],
        Endpoint::SetQuota => vec![Stmt::SetQuota(Box::new(ddl::set_quota(body)?))],
    })
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
    }
}
