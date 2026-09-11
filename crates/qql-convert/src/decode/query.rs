//! Decode `QueryRequest` / `QueryGroupsRequest` into
//! [`qql_core::ast::QueryStmt`] values.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::{
    DecodeCtx, filter,
    interface::query_interface,
    params::{decode_params, decode_with_payload, decode_with_vector},
    prefetch::{attach_prefetch, attach_using, expression_prefetch, prefetch_field},
    vector,
};
use crate::json::{self, child, invalid};
use qql_core::ast::{
    Cte, GroupSpec, PageSpec, QueryCollection, QueryExpr, QueryOutput, QueryStmt, SearchParams,
};

const QUERY_KEYS: &[&str] = &[
    "query",
    "prefetch",
    "filter",
    "params",
    "using",
    "limit",
    "offset",
    "score_threshold",
    "with_payload",
    "with_vector",
    "shard_key",
    "lookup_from",
];

const QUERY_GROUP_KEYS: &[&str] = &[
    "query",
    "prefetch",
    "filter",
    "params",
    "using",
    "limit",
    "offset",
    "score_threshold",
    "with_payload",
    "with_vector",
    "shard_key",
    "lookup_from",
    "group_by",
    "group_size",
    "with_lookup",
];

/// Decode a `POST /points/query` body.
pub(crate) fn query_request(body: &Value, ctx: DecodeCtx<'_>) -> Result<QueryStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, QUERY_KEYS)?;
    let stmt = base_query(obj, path, ctx)?;
    validate_lookup_from(obj, &stmt.expression, path)?;
    Ok(stmt)
}

/// Decode a `POST /points/query/groups` body.
pub(crate) fn query_groups_request(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<QueryStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, QUERY_GROUP_KEYS)?;
    let mut stmt = base_query(obj, path, ctx)?;
    validate_lookup_from(obj, &stmt.expression, path)?;

    let group_by = json::required_str(obj, "group_by", path)?;
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
            Some(json::required_str(
                lookup,
                "collection",
                &child(path, "with_lookup"),
            )?)
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

/// Shared query-expression + filter + params + paging decode.
pub(crate) struct CoreQuery {
    pub expression: QueryExpr,
    pub filter: Option<Box<qql_core::ast::FilterExpr>>,
    pub params: Option<SearchParams>,
    pub score_threshold: Option<f64>,
    pub limit: Option<u64>,
}

/// Decode the fields shared by a query request and a prefetch stage.
pub(crate) fn core_query(
    obj: &json::Obj,
    path: &str,
    missing_query: ConvertError,
) -> Result<CoreQuery, ConvertError> {
    let prefetches = prefetch_field(obj, path)?;
    let limit = json::opt_u64(obj, "limit", path)?;
    let Some(query) = obj.get("query").filter(|v| !v.is_null()) else {
        return Err(missing_query);
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

    Ok(CoreQuery {
        expression,
        filter: filter::filter_field(obj, "filter", path)?,
        params,
        score_threshold: json::opt_f64(obj, "score_threshold", path)?,
        limit,
    })
}

/// Decode the fields shared by both query endpoints.
fn base_query(obj: &json::Obj, path: &str, ctx: DecodeCtx<'_>) -> Result<QueryStmt, ConvertError> {
    let mut core = core_query(
        obj,
        path,
        ConvertError::undecodable(
            "QUERY request has no `query` field; QQL cannot express an ID-ordered scan",
        ),
    )?;
    ctx.opts.apply_read(&mut core.params);
    Ok(QueryStmt {
        ctes: Vec::<Cte>::new(),
        collection: QueryCollection::Explicit(ctx.collection.to_string()),
        expression: core.expression,
        filter: core.filter,
        params: core.params,
        score_threshold: core.score_threshold,
        group: None,
        output: QueryOutput {
            payload: decode_with_payload(obj, path)?,
            vectors: decode_with_vector(obj, path)?,
        },
        page: PageSpec {
            limit: core.limit,
            offset: json::opt_u64(obj, "offset", path)?,
            ..PageSpec::default()
        },
        shard_key: vector::shard_key_field(obj, path)?,
    })
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
