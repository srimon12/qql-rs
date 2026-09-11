//! Decode `QueryRequest` / `QueryGroupsRequest` into
//! [`qql_core::ast::QueryStmt`] values.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::{
    filter,
    interface::query_interface,
    params::{decode_params, decode_with_payload, decode_with_vector},
    prefetch::{attach_prefetch, attach_using, expression_prefetch, prefetch_field},
    vector,
};
use crate::json::{self, child, invalid};
use qql_core::ast::{
    Cte, GroupSpec, PageSpec, QueryCollection, QueryExpr, QueryOutput, QueryStmt, SearchParams,
};

/// Decode a `POST /points/query` body.
pub(crate) fn query_request(body: &Value, collection: &str) -> Result<QueryStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    let stmt = base_query(obj, path, collection)?;
    validate_lookup_from(obj, &stmt.expression, path)?;
    Ok(stmt)
}

/// Decode a `POST /points/query/groups` body.
pub(crate) fn query_groups_request(
    body: &Value,
    collection: &str,
) -> Result<QueryStmt, ConvertError> {
    let path = "body";
    let obj = json::object(body, path)?;
    let mut stmt = base_query(obj, path, collection)?;
    validate_lookup_from(obj, &stmt.expression, path)?;

    let group_by = json::required(obj, "group_by", path)
        .and_then(|v| Ok(json::string_at(v, &child(path, "group_by"))?.to_string()))?;
    if group_by.is_empty() {
        return Err(invalid(
            child(path, "group_by"),
            "group_by must not be empty",
        ));
    }
    let size = json::opt_u64(obj, "group_size", path)?;
    let lookup = match obj.get("with_lookup").filter(|v| !v.is_null()) {
        None => None,
        Some(Value::String(name)) => Some(name.clone()),
        Some(Value::Object(lookup)) => {
            for key in lookup.keys() {
                if key != "collection" {
                    return Err(invalid(
                        child(&child(path, "with_lookup"), key),
                        "group lookup selectors (with_payload / with_vectors) have no QQL representation",
                    ));
                }
            }
            Some(
                json::required(lookup, "collection", &child(path, "with_lookup")).and_then(
                    |v| {
                        Ok(
                            json::string_at(v, &child(&child(path, "with_lookup"), "collection"))?
                                .to_string(),
                        )
                    },
                )?,
            )
        }
        Some(other) => {
            return Err(invalid(
                child(path, "with_lookup"),
                format!("expected a collection name, got {}", json::type_name(other)),
            ));
        }
    };
    stmt.group = Some(GroupSpec {
        field: group_by,
        size,
        lookup,
    });
    Ok(stmt)
}

/// Decode the fields shared by both query endpoints.
fn base_query(obj: &json::Obj, path: &str, collection: &str) -> Result<QueryStmt, ConvertError> {
    let prefetches = prefetch_field(obj, path)?;
    let limit = json::opt_u64(obj, "limit", path)?;
    let Some(query) = obj.get("query").filter(|v| !v.is_null()) else {
        return Err(ConvertError::undecodable(
            "QUERY request has no `query` field; QQL cannot express an ID-ordered scan",
        ));
    };
    let interface = query_interface(query, &child(path, "query"), limit)?;

    let using = json::opt_string(obj, "using", path)?;
    let mut expression = attach_using(interface.expression, using.as_deref(), path)?;
    expression = attach_prefetch(expression, prefetches, path)?;

    let mut params = match obj.get("params").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => decode_params(value, &child(path, "params"))?,
    };
    if let Some(rrf) = interface.rrf {
        let params = params.get_or_insert_with(SearchParams::default);
        params.rrf_k = rrf.k;
        params.rrf_weights = rrf.weights;
    }

    Ok(QueryStmt {
        ctes: Vec::<Cte>::new(),
        collection: QueryCollection::Explicit(collection.to_string()),
        expression,
        filter: filter::filter_field(obj, "filter", path)?,
        params,
        score_threshold: json::opt_f64(obj, "score_threshold", path)?,
        group: None,
        output: QueryOutput {
            payload: decode_with_payload(obj, path)?,
            vectors: decode_with_vector(obj, path)?,
        },
        page: PageSpec {
            limit,
            offset: json::opt_u64(obj, "offset", path)?,
            ..PageSpec::default()
        },
        shard_key: decode_shard_key_field(obj, path)?,
    })
}

/// Decode the optional `shard_key` request member.
pub(crate) fn decode_shard_key_field(
    obj: &json::Obj,
    path: &str,
) -> Result<Option<qql_core::ast::ShardKey>, ConvertError> {
    match obj.get("shard_key").filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(value) => Ok(Some(vector::shard_key(value, &child(path, "shard_key"))?)),
    }
}

/// Check that a top-level `lookup_from` can be reproduced by the planner.
///
/// The planner derives `QueryRequest.lookup_from` from the *first* prefetch
/// carrying a `LOOKUP FROM`; QQL has no standalone top-level clause.
fn validate_lookup_from(
    obj: &json::Obj,
    expression: &QueryExpr,
    path: &str,
) -> Result<(), ConvertError> {
    let Some(value) = obj.get("lookup_from").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let expected = vector::lookup_spec(value, &child(path, "lookup_from"))?;
    let found = expression_prefetch(expression)
        .iter()
        .find_map(|prefetch| prefetch.lookup.clone());
    if found.as_ref() == Some(&expected) {
        Ok(())
    } else {
        Err(invalid(
            child(path, "lookup_from"),
            "top-level lookup_from is only reproducible from a PREFETCH LOOKUP FROM clause",
        ))
    }
}
