//! Decode `SearchParams`, payload/vector selectors, and IDF scope.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::filter;
use crate::json::{self, child, index, invalid};
use qql_core::ast::{
    IdfParams, PayloadSelector, QuantizationSearchParams, SearchParams, VectorSelector,
};

const PARAMS_KEYS: &[&str] = &[
    "hnsw_ef",
    "exact",
    "indexed_only",
    "acorn",
    "quantization",
    "idf",
];

/// Decode a `SearchParams` body.
pub(crate) fn decode_params(
    value: &Value,
    path: &str,
) -> Result<Option<SearchParams>, ConvertError> {
    let obj = json::object(value, path)?;
    json::reject_unknown(obj, path, PARAMS_KEYS)?;
    let mut params = SearchParams {
        hnsw_ef: json::opt_u64(obj, "hnsw_ef", path)?,
        exact: json::opt_bool(obj, "exact", path)?,
        indexed_only: json::opt_bool(obj, "indexed_only", path)?,
        ..SearchParams::default()
    };
    if let Some(acorn) = json::opt_object(obj, "acorn", path)? {
        params.acorn =
            Some(json::opt_bool(acorn, "enable", &child(path, "acorn"))?.unwrap_or(false));
        params.max_selectivity = json::opt_f64(acorn, "max_selectivity", &child(path, "acorn"))?;
    }
    if let Some(quantization) = json::opt_object(obj, "quantization", path)? {
        params.quantization = Some(QuantizationSearchParams {
            ignore: json::opt_bool(quantization, "ignore", &child(path, "quantization"))?,
            rescore: json::opt_bool(quantization, "rescore", &child(path, "quantization"))?,
            oversampling: json::opt_f64(
                quantization,
                "oversampling",
                &child(path, "quantization"),
            )?,
        });
    }
    if let Some(idf) = obj.get("idf").filter(|v| !v.is_null()) {
        let idf_path = child(path, "idf");
        params.idf = Some(match idf {
            Value::String(scope) if scope == "global" => IdfParams { corpus: None },
            Value::Object(idf_obj) if idf_obj.contains_key("corpus") => IdfParams {
                corpus: Some(filter::filter(
                    idf_obj.get("corpus").expect("key checked"),
                    &child(&idf_path, "corpus"),
                )?),
            },
            other => {
                return Err(invalid(
                    idf_path,
                    format!(
                        "expected \"global\" or a corpus filter, got {}",
                        json::type_name(other)
                    ),
                ));
            }
        });
    }
    if params == SearchParams::default() {
        // An empty object carries no knobs; QQL has no `PARAMS ()`.
        return Ok(None);
    }
    Ok(Some(params))
}

/// Decode `with_payload` into a payload selector.
pub(crate) fn decode_with_payload(
    obj: &json::Obj,
    path: &str,
) -> Result<Option<PayloadSelector>, ConvertError> {
    let Some(value) = obj.get("with_payload").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let w_path = child(path, "with_payload");
    match value {
        // REST `true` is QQL's default (payloads included); omit the clause.
        Value::Bool(true) => Ok(None),
        Value::Bool(false) => Ok(Some(PayloadSelector::None)),
        Value::Array(items) => {
            let names = payload_names(items, &w_path)?;
            if names.is_empty() {
                return Err(invalid(
                    w_path,
                    "an empty payload include list selects no fields and has no QQL representation",
                ));
            }
            Ok(Some(PayloadSelector::Include(names)))
        }
        Value::Object(selector) => {
            for key in selector.keys() {
                match key.as_str() {
                    "include" => {}
                    "exclude" => {}
                    other => {
                        return Err(invalid(
                            child(&w_path, other),
                            "unknown with_payload selector",
                        ));
                    }
                }
            }
            let names = |key: &str| -> Result<Vec<String>, ConvertError> {
                let list_path = child(&w_path, key);
                let raw = json::required(selector, key, &w_path)?
                    .as_array()
                    .ok_or_else(|| {
                        invalid(list_path.clone(), "expected an array of field names")
                    })?;
                payload_names(raw, &list_path)
            };
            match (
                selector.contains_key("include"),
                selector.contains_key("exclude"),
            ) {
                (true, false) => {
                    let fields = names("include")?;
                    if fields.is_empty() {
                        return Err(invalid(
                            child(&w_path, "include"),
                            "an empty payload include list selects no fields and has no QQL representation",
                        ));
                    }
                    Ok(Some(PayloadSelector::Include(fields)))
                }
                (false, true) => {
                    let fields = names("exclude")?;
                    if fields.is_empty() {
                        // Excluding nothing keeps every field — the QQL default.
                        return Ok(None);
                    }
                    Ok(Some(PayloadSelector::Exclude(fields)))
                }
                _ => Err(invalid(
                    w_path,
                    "with_payload selector must set exactly one of include / exclude",
                )),
            }
        }
        other => Err(invalid(
            w_path,
            format!(
                "expected a boolean, field list, or include/exclude object, got {}",
                json::type_name(other)
            ),
        )),
    }
}

/// Decode `with_vector` into a vector selector.
pub(crate) fn decode_with_vector(
    obj: &json::Obj,
    path: &str,
) -> Result<Option<VectorSelector>, ConvertError> {
    let Some(value) = obj.get("with_vector").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let w_path = child(path, "with_vector");
    match value {
        Value::Bool(true) => Ok(Some(VectorSelector::All)),
        Value::Bool(false) => Ok(Some(VectorSelector::None)),
        Value::Array(items) => {
            let names = payload_names(items, &w_path)?;
            if names.is_empty() {
                // Selecting no names is exactly `WITH VECTOR false`.
                return Ok(Some(VectorSelector::None));
            }
            Ok(Some(VectorSelector::Names(names)))
        }
        other => Err(invalid(
            w_path,
            format!(
                "expected a boolean or vector-name list, got {}",
                json::type_name(other)
            ),
        )),
    }
}

/// Decode a JSON array of field / vector names.
fn payload_names(items: &[Value], path: &str) -> Result<Vec<String>, ConvertError> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| Ok(json::string_at(item, &index(path, i))?.to_string()))
        .collect()
}
