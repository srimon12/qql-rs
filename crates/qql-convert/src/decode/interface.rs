//! Decode the `QueryInterface` union: nearest / recommend / context / discover
//! and their shared example lists.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::modes::{
    formula_query, fusion, order_by, relevance_feedback, rrf_params, sample,
};
use crate::decode::vector;
use crate::json::{self, child, index, invalid};
use qql_core::ast::{FusionMethod, MmrConfig, QueryExpr, QueryInput, RecommendStrategy};

/// RRF parameters carried by the `{"rrf": …}` query variant.
pub(crate) struct RrfParams {
    pub(crate) k: Option<u64>,
    pub(crate) weights: Option<Vec<f64>>,
}

/// The decoded query interface union member plus carried RRF parameters.
pub(crate) struct Interface {
    pub(crate) expression: QueryExpr,
    pub(crate) rrf: Option<RrfParams>,
}

/// Decode a `QueryInterface` value (`Query` union or bare `VectorInput`).
pub(crate) fn query_interface(
    value: &Value,
    path: &str,
    fallback_limit: Option<u64>,
) -> Result<Interface, ConvertError> {
    let Value::Object(obj) = value else {
        // Bare VectorInput: dense/sparse/multi vector, point id, document, image.
        return Ok(Interface {
            expression: QueryExpr::Nearest {
                input: vector::query_input(value, path)?,
                using: None,
                prefetch: Vec::new(),
                mmr: None,
            },
            rrf: None,
        });
    };

    let known: Vec<&str> = [
        "nearest",
        "recommend",
        "context",
        "discover",
        "order_by",
        "sample",
        "fusion",
        "rrf",
        "formula",
        "relevance_feedback",
    ]
    .into_iter()
    .filter(|key| obj.contains_key(*key))
    .collect();
    if known.len() > 1 {
        return Err(invalid(
            path,
            format!("query object mixes multiple variants: {}", known.join(", ")),
        ));
    }
    let Some(kind) = known.first() else {
        // Not a Query union member: document / image / sparse VectorInput.
        return Ok(Interface {
            expression: QueryExpr::Nearest {
                input: vector::query_input(value, path)?,
                using: None,
                prefetch: Vec::new(),
                mmr: None,
            },
            rrf: None,
        });
    };

    let expression = match *kind {
        "nearest" => nearest(obj, path, fallback_limit)?,
        "recommend" => recommend(obj, path)?,
        "context" => context(obj, path)?,
        "discover" => discover(obj, path)?,
        "order_by" => order_by(obj, path)?,
        "sample" => sample(obj, path)?,
        "fusion" => fusion(obj, path)?,
        "formula" => formula_query(obj, path)?,
        "relevance_feedback" => relevance_feedback(obj, path)?,
        "rrf" => {
            let rrf = rrf_params(obj, path)?;
            return Ok(Interface {
                expression: QueryExpr::Fusion {
                    method: FusionMethod::Rrf,
                    prefetch: Vec::new(),
                },
                rrf: Some(rrf),
            });
        }
        other => unreachable!("known key {other}"),
    };
    Ok(Interface {
        expression,
        rrf: None,
    })
}

/// Decode `{"nearest": VectorInput, "mmr": …}`.
fn nearest(
    obj: &json::Obj,
    path: &str,
    _fallback_limit: Option<u64>,
) -> Result<QueryExpr, ConvertError> {
    let nearest_path = child(path, "nearest");
    let input = vector::query_input(json::required(obj, "nearest", path)?, &nearest_path)?;
    let mmr = match obj.get("mmr").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => {
            let mmr_path = child(path, "mmr");
            let mmr = json::object(value, &mmr_path)?;
            json::reject_unknown(mmr, &mmr_path, &["diversity", "candidates_limit"])?;
            let diversity = match mmr.get("diversity").filter(|v| !v.is_null()) {
                Some(v) => json::f64_at(v, &child(&mmr_path, "diversity"))?,
                None => {
                    return Err(invalid(
                        child(&mmr_path, "diversity"),
                        "mmr requires diversity and candidates_limit",
                    ));
                }
            };
            let candidates = match mmr.get("candidates_limit").filter(|v| !v.is_null()) {
                Some(v) => json::u64_at(v, &child(&mmr_path, "candidates_limit"))?,
                None => {
                    return Err(invalid(
                        child(&mmr_path, "candidates_limit"),
                        "mmr requires diversity and candidates_limit",
                    ));
                }
            };
            Some(Box::new(MmrConfig {
                diversity,
                candidates,
            }))
        }
    };
    Ok(QueryExpr::Nearest {
        input,
        using: None,
        prefetch: Vec::new(),
        mmr,
    })
}

/// Decode `{"recommend": {positive, negative, strategy}}`.
fn recommend(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let rec_path = child(path, "recommend");
    let rec = json::object(json::required(obj, "recommend", path)?, &rec_path)?;
    let positive = vector_list(rec, "positive", &rec_path)?;
    let negative = vector_list(rec, "negative", &rec_path)?;
    let strategy = match rec.get("strategy").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(
            match json::string_at(value, &child(&rec_path, "strategy"))? {
                "average_vector" => RecommendStrategy::AverageVector,
                "best_score" => RecommendStrategy::BestScore,
                "sum_scores" => RecommendStrategy::SumScores,
                other => {
                    return Err(invalid(
                        child(&rec_path, "strategy"),
                        format!("unknown recommend strategy '{other}'"),
                    ));
                }
            },
        ),
    };
    if positive.is_empty() {
        return Err(invalid(
            child(&rec_path, "positive"),
            "QQL RECOMMEND requires at least one positive example",
        ));
    }
    Ok(QueryExpr::Recommend {
        positive,
        negative,
        strategy,
        using: None,
        prefetch: Vec::new(),
    })
}

/// Decode `{"context": pair | [pair, …]}`.
fn context(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let ctx_path = child(path, "context");
    let pairs = context_pairs(
        json::required(obj, "context", path)?,
        &ctx_path,
        "QQL CONTEXT requires at least one pair",
    )?;
    Ok(QueryExpr::Context {
        pairs,
        using: None,
        prefetch: Vec::new(),
    })
}

/// Decode `{"discover": {target, context}}`.
fn discover(obj: &json::Obj, path: &str) -> Result<QueryExpr, ConvertError> {
    let dis_path = child(path, "discover");
    let dis = json::object(json::required(obj, "discover", path)?, &dis_path)?;
    let target = vector::query_input(
        json::required(dis, "target", &dis_path)?,
        &child(&dis_path, "target"),
    )?;
    let pairs = context_pairs(
        json::required(dis, "context", &dis_path)?,
        &child(&dis_path, "context"),
        "QQL DISCOVER requires at least one context pair",
    )?;
    Ok(QueryExpr::Discover {
        target,
        context: pairs,
        using: None,
        prefetch: Vec::new(),
    })
}

/// Decode a list of `VectorInput` examples.
pub(crate) fn vector_list(
    obj: &json::Obj,
    key: &str,
    path: &str,
) -> Result<Vec<QueryInput>, ConvertError> {
    match obj.get(key).filter(|v| !v.is_null()) {
        None => Ok(Vec::new()),
        Some(value) => {
            let list_path = child(path, key);
            json::array(value, &list_path)?
                .iter()
                .enumerate()
                .map(|(i, item)| vector::query_input(item, &index(&list_path, i)))
                .collect()
        }
    }
}

/// Decode `ContextInput`: a single pair or an array of pairs.
pub(crate) fn context_pairs(
    value: &Value,
    path: &str,
    empty_detail: &str,
) -> Result<Vec<qql_core::ast::ContextPair>, ConvertError> {
    let items: Vec<Value> = match value {
        Value::Array(items) => items.clone(),
        single => vec![single.clone()],
    };
    let mut pairs = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let pair_path = index(path, i);
        let pair = json::object(item, &pair_path)?;
        pairs.push(qql_core::ast::ContextPair {
            positive: vector::query_input(
                json::required(pair, "positive", &pair_path)?,
                &child(&pair_path, "positive"),
            )?,
            negative: vector::query_input(
                json::required(pair, "negative", &pair_path)?,
                &child(&pair_path, "negative"),
            )?,
        });
    }
    if pairs.is_empty() {
        return Err(invalid(path, empty_detail));
    }
    Ok(pairs)
}
