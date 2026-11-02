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
    /// Server BM25 scorer params carried by a Qdrant 1.20 `TextQuery`
    /// (`text.scoring.bm25`). They lower to `PARAMS (k, b)` on `QUERY BM25`.
    pub(crate) bm25: Option<Bm25Params>,
}

/// `Bm25Params` from a `TextQuery`'s `scoring.bm25` (both optional).
#[derive(Default)]
pub(crate) struct Bm25Params {
    pub(crate) k: Option<f64>,
    pub(crate) b: Option<f64>,
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
            bm25: None,
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
        // Qdrant 1.20 `TextQuery` (`{"text": "…"}` / `{"text": {query,
        // scoring}}`) shares the `text` key with the inference `Document`,
        // which always carries `model`. Model-less means server-side BM25
        // over a text index → `QUERY BM25`, never the embed `TEXT` path.
        if obj.contains_key("text") && !obj.contains_key("model") {
            let (text, bm25) = text_query(obj, path)?;
            return Ok(Interface {
                expression: QueryExpr::Bm25 {
                    text,
                    field: String::new(),
                    prefetch: Vec::new(),
                    text_param: None,
                },
                rrf: None,
                bm25: Some(bm25),
            });
        }
        // Not a Query union member: document / image / sparse VectorInput.
        return Ok(Interface {
            expression: QueryExpr::Nearest {
                input: vector::query_input(value, path)?,
                using: None,
                prefetch: Vec::new(),
                mmr: None,
            },
            rrf: None,
            bm25: None,
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
                bm25: None,
            });
        }
        // Drift guard: `kind` comes from the fixed `known` key table above,
        // so every member is covered. Fail closed (never panic) if the key
        // table and this match ever drift apart.
        other => {
            return Err(invalid(
                path,
                format!("unsupported query interface variant '{other}'"),
            ));
        }
    };
    Ok(Interface {
        expression,
        rrf: None,
        bm25: None,
    })
}

/// Decode a Qdrant 1.20 `TextQuery`: `{"text": TextInterface}` where
/// `TextInterface` is a bare string or `{query, scoring?: {bm25: {k?, b?}}}`.
fn text_query(obj: &json::Obj, path: &str) -> Result<(String, Bm25Params), ConvertError> {
    json::reject_unknown(obj, path, &["text"])?;
    let text_path = child(path, "text");
    let value = json::required(obj, "text", path)?;
    let (text, params) = match value {
        Value::String(text) => (text.clone(), Bm25Params::default()),
        Value::Object(input) => {
            json::reject_unknown(input, &text_path, &["query", "scoring"])?;
            let text = json::string_at(
                json::required(input, "query", &text_path)?,
                &child(&text_path, "query"),
            )?
            .to_string();
            let params = match input.get("scoring").filter(|v| !v.is_null()) {
                None => Bm25Params::default(),
                Some(scoring) => bm25_scoring(scoring, &child(&text_path, "scoring"))?,
            };
            (text, params)
        }
        other => {
            return Err(invalid(
                text_path,
                format!(
                    "expected a text query string or {{query, scoring}} object, got {}",
                    json::type_name(other)
                ),
            ));
        }
    };
    if text.trim().is_empty() {
        return Err(invalid(text_path, "BM25 text query must not be empty"));
    }
    Ok((text, params))
}

/// Decode `TextQueryScoring` (`{"bm25": {k?, b?}}`), mirroring the parser's
/// bounds: `k >= 0`, `b` in `[0, 1]`.
fn bm25_scoring(value: &Value, path: &str) -> Result<Bm25Params, ConvertError> {
    let obj = json::object(value, path)?;
    json::reject_unknown(obj, path, &["bm25"])?;
    let bm25_path = child(path, "bm25");
    let Some(params) = obj.get("bm25").filter(|v| !v.is_null()) else {
        return Ok(Bm25Params::default());
    };
    let params = json::object(params, &bm25_path)?;
    json::reject_unknown(params, &bm25_path, &["k", "b"])?;
    let k = json::opt_f64(params, "k", &bm25_path)?;
    let b = json::opt_f64(params, "b", &bm25_path)?;
    if let Some(k) = k
        && (!k.is_finite() || k < 0.0)
    {
        return Err(invalid(child(&bm25_path, "k"), "BM25 k must be >= 0"));
    }
    if let Some(b) = b
        && (!b.is_finite() || !(0.0..=1.0).contains(&b))
    {
        return Err(invalid(
            child(&bm25_path, "b"),
            "BM25 b must be within [0, 1]",
        ));
    }
    Ok(Bm25Params { k, b })
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
