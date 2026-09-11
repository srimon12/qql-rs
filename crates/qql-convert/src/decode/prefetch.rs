//! Decode `PREFETCH` stages into inline `PREFETCH (QUERY …)` clauses and
//! attach `USING` / prefetch lists to query expressions.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::query::core_query;
use crate::decode::vector;
use crate::json::{self, child, index, invalid};
use qql_core::ast::{
    PageSpec, Prefetch, PrefetchSource, QueryCollection, QueryExpr, QueryOutput, QueryStmt,
    VectorTarget,
};

/// Decode the optional `prefetch` member (single stage or list).
pub(crate) fn prefetch_field(obj: &json::Obj, path: &str) -> Result<Vec<Prefetch>, ConvertError> {
    let Some(value) = obj.get("prefetch").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let list_path = child(path, "prefetch");
    match value {
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| prefetch(item, &index(&list_path, i)))
            .collect(),
        single => Ok(vec![prefetch(single, &list_path)?]),
    }
}

/// Decode one `Prefetch` stage into an inline `PREFETCH (QUERY …)` clause.
///
/// Filter, limit, score threshold, and params live on the inline query; nested
/// stages recurse into its `PREFETCH` tail. This matches
/// `qql_plan::prefetch::lower_prefetch_with_ctes` exactly for inline sources.
const PREFETCH_KEYS: &[&str] = &[
    "query",
    "prefetch",
    "filter",
    "params",
    "using",
    "limit",
    "score_threshold",
    "lookup_from",
];

fn prefetch(value: &Value, path: &str) -> Result<Prefetch, ConvertError> {
    let obj = json::object(value, path)?;
    json::reject_unknown(obj, path, PREFETCH_KEYS)?;
    let core = core_query(
        obj,
        path,
        invalid(
            child(path, "query"),
            "filter-only prefetch stages have no QQL representation (PREFETCH requires a QUERY)",
        ),
    )?;

    let stmt = QueryStmt {
        ctes: Vec::new(),
        collection: QueryCollection::Inherited,
        expression: core.expression,
        filter: core.filter,
        params: core.params,
        score_threshold: core.score_threshold,
        group: None,
        output: QueryOutput::default(),
        page: PageSpec {
            limit: core.limit,
            offset: None,
            ..PageSpec::default()
        },
        shard_key: None,
    };
    let lookup = match obj.get("lookup_from").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(vector::lookup_spec(value, &child(path, "lookup_from"))?),
    };
    Ok(Prefetch {
        source: PrefetchSource::Query(Box::new(stmt)),
        filter: None,
        score_threshold: None,
        lookup,
    })
}

/// Attach a `USING` target to the expression kinds that carry one.
pub(crate) fn attach_using(
    expression: QueryExpr,
    using: Option<&str>,
    path: &str,
) -> Result<QueryExpr, ConvertError> {
    let Some(name) = using else {
        return Ok(expression);
    };
    let target = || VectorTarget {
        name: name.to_string(),
        kind: None,
        multi: false,
    };
    Ok(match expression {
        QueryExpr::Nearest {
            input,
            prefetch,
            mmr,
            ..
        } => QueryExpr::Nearest {
            input,
            using: Some(target()),
            prefetch,
            mmr,
        },
        QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            prefetch,
            ..
        } => QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            using: Some(target()),
            prefetch,
        },
        QueryExpr::Context {
            pairs, prefetch, ..
        } => QueryExpr::Context {
            pairs,
            using: Some(target()),
            prefetch,
        },
        QueryExpr::Discover {
            target: anchor,
            context,
            prefetch,
            ..
        } => QueryExpr::Discover {
            target: anchor,
            context,
            using: Some(target()),
            prefetch,
        },
        QueryExpr::RelevanceFeedback {
            target: anchor,
            feedback,
            strategy,
            prefetch,
            ..
        } => QueryExpr::RelevanceFeedback {
            target: anchor,
            feedback,
            strategy,
            using: Some(target()),
            prefetch,
        },
        other => {
            let _ = other;
            return Err(invalid(
                child(path, "using"),
                "USING cannot target this query expression in QQL",
            ));
        }
    })
}

/// Attach prefetch stages to the expression kinds that carry them.
pub(crate) fn attach_prefetch(
    expression: QueryExpr,
    prefetch: Vec<Prefetch>,
    path: &str,
) -> Result<QueryExpr, ConvertError> {
    if prefetch.is_empty() {
        return Ok(expression);
    }
    Ok(match expression {
        QueryExpr::Nearest {
            input, using, mmr, ..
        } => QueryExpr::Nearest {
            input,
            using,
            prefetch,
            mmr,
        },
        QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            using,
            ..
        } => QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            using,
            prefetch,
        },
        QueryExpr::Context { pairs, using, .. } => QueryExpr::Context {
            pairs,
            using,
            prefetch,
        },
        QueryExpr::Discover {
            target,
            context,
            using,
            ..
        } => QueryExpr::Discover {
            target,
            context,
            using,
            prefetch,
        },
        QueryExpr::Fusion { method, .. } => QueryExpr::Fusion { method, prefetch },
        QueryExpr::Formula {
            expression: formula,
            defaults,
            ..
        } => QueryExpr::Formula {
            expression: formula,
            defaults,
            prefetch,
        },
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            using,
            ..
        } => QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            using,
            prefetch,
        },
        other => {
            let _ = other;
            return Err(invalid(
                child(path, "prefetch"),
                "PREFETCH is not supported for this query expression in QQL",
            ));
        }
    })
}

/// Prefetch stages carried by a query expression.
pub(crate) fn expression_prefetch(expression: &QueryExpr) -> &[Prefetch] {
    match expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => prefetch,
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => &[],
    }
}
