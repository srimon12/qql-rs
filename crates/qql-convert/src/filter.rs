//! REST filter/condition lowering to QQL `WHERE` clauses.

use serde_json::Value;

use crate::ConvertError;
use crate::formatters::{escape_qql_string, format_value};
use crate::sanitize::format_id;

/// Lower a REST `filter` object to a QQL boolean expression.
///
/// Returns an empty string when the filter carries no conditions.
pub(crate) fn convert_filter_to_qql(filter: &Value) -> Result<String, ConvertError> {
    if let Some(obj) = filter.as_object() {
        let mut parts = Vec::new();

        if let Some(must) = obj.get("must").and_then(|v| v.as_array()) {
            for cond in must {
                let s = convert_condition(cond)?;
                if !s.is_empty() {
                    parts.push(s);
                }
            }
        }

        if let Some(should) = obj.get("should").and_then(|v| v.as_array()) {
            if should.len() == 1 {
                let s = convert_condition(&should[0])?;
                if !s.is_empty() {
                    parts.push(s);
                }
            } else if should.len() > 1 {
                let mut inner = Vec::new();
                for cond in should {
                    let s = convert_condition(cond)?;
                    if !s.is_empty() {
                        inner.push(s);
                    }
                }
                if inner.len() == 1 {
                    parts.push(inner.into_iter().next().unwrap());
                } else if !inner.is_empty() {
                    parts.push(format!("({})", inner.join(" OR ")));
                }
            }
        }

        if let Some(must_not) = obj.get("must_not").and_then(|v| v.as_array()) {
            for cond in must_not {
                let s = convert_condition(cond)?;
                if !s.is_empty() {
                    parts.push(format!("NOT ({s})"));
                }
            }
        }

        if parts.is_empty() {
            return Ok(String::new());
        }
        if parts.len() == 1 {
            Ok(parts.into_iter().next().unwrap())
        } else {
            Ok(format!("({})", parts.join(" AND ")))
        }
    } else {
        Ok(String::new())
    }
}

/// Lower a single REST filter condition to QQL.
///
/// Returns [`ConvertError::GeoUnsupported`] for geo predicates, which have
/// no QQL representation.
pub(crate) fn convert_condition(cond: &Value) -> Result<String, ConvertError> {
    let obj = match cond.as_object() {
        Some(o) => o,
        None => return Ok(String::new()),
    };

    if let Some(has_id) = obj.get("has_id").and_then(|v| v.as_array()) {
        let ids: Vec<String> = has_id.iter().map(format_id).collect();
        return Ok(format!("id IN ({})", ids.join(", ")));
    }

    if let Some(is_empty) = obj.get("is_empty").and_then(|v| v.as_object())
        && let Some(key) = is_empty.get("key").and_then(|v| v.as_str())
    {
        return Ok(format!("{key} IS EMPTY"));
    }

    if let Some(is_null) = obj.get("is_null").and_then(|v| v.as_object())
        && let Some(key) = is_null.get("key").and_then(|v| v.as_str())
    {
        return Ok(format!("{key} IS NULL"));
    }

    let key = obj.get("key").and_then(|v| v.as_str()).unwrap_or("");

    if let Some(match_obj) = obj.get("match").and_then(|v| v.as_object()) {
        if let Some(value) = match_obj.get("value") {
            return Ok(format!("{key} = {}", format_value(value)));
        }
        if let Some(keyword) = match_obj.get("keyword") {
            return Ok(format!("{key} = {}", format_value(keyword)));
        }
        if let Some(integer) = match_obj.get("integer") {
            return Ok(format!("{key} = {}", format_value(integer)));
        }
        if let Some(boolean) = match_obj.get("boolean") {
            return Ok(format!("{key} = {}", format_value(boolean)));
        }
        if let Some(text) = match_obj.get("text").and_then(|v| v.as_str()) {
            return Ok(format!("{key} MATCH '{}'", escape_qql_string(text)));
        }
        if let Some(text_any) = match_obj.get("text_any").and_then(|v| v.as_str()) {
            return Ok(format!("{key} MATCH ANY '{}'", escape_qql_string(text_any)));
        }
        if let Some(any) = match_obj.get("any").and_then(|v| v.as_array()) {
            let vals: Vec<String> = any.iter().map(format_value).collect();
            return Ok(format!("{key} IN ({})", vals.join(", ")));
        }
        if let Some(except) = match_obj.get("except").and_then(|v| v.as_array()) {
            let vals: Vec<String> = except.iter().map(format_value).collect();
            return Ok(format!("{key} NOT IN ({})", vals.join(", ")));
        }
    }

    if let Some(range) = obj.get("range").and_then(|v| v.as_object()) {
        let gte = range.get("gte");
        let gt = range.get("gt");
        let lte = range.get("lte");
        let lt = range.get("lt");

        if let (Some(low), Some(high)) = (gte, lte) {
            return Ok(format!(
                "{key} BETWEEN {} AND {}",
                format_value(low),
                format_value(high)
            ));
        }
        if let (Some(low), Some(high)) = (gte.or(gt), lte.or(lt)) {
            let op_low = if gte.is_some() { ">=" } else { ">" };
            let op_high = if lte.is_some() { "<=" } else { "<" };
            return Ok(format!(
                "{key} {op_low} {} AND {key} {op_high} {}",
                format_value(low),
                format_value(high)
            ));
        }
        if let Some(v) = gte {
            return Ok(format!("{key} >= {}", format_value(v)));
        }
        if let Some(v) = gt {
            return Ok(format!("{key} > {}", format_value(v)));
        }
        if let Some(v) = lte {
            return Ok(format!("{key} <= {}", format_value(v)));
        }
        if let Some(v) = lt {
            return Ok(format!("{key} < {}", format_value(v)));
        }
    }

    if obj
        .get("geo_bounding_box")
        .and_then(|v| v.as_object())
        .is_some()
    {
        return Err(ConvertError::GeoUnsupported("geo_bounding_box"));
    }

    if obj.get("geo_radius").and_then(|v| v.as_object()).is_some() {
        return Err(ConvertError::GeoUnsupported("geo_radius"));
    }

    if obj.get("geo_polygon").and_then(|v| v.as_object()).is_some() {
        return Err(ConvertError::GeoUnsupported("geo_polygon"));
    }

    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::convert_condition;
    use crate::ConvertError;

    #[test]
    fn range_conversion_handles_inclusive_and_strict_inequalities() {
        let between_res = convert_condition(&json!({
            "key": "age",
            "range": { "gte": 18, "lte": 65 }
        }))
        .expect("between condition");
        assert_eq!(between_res, "age BETWEEN 18 AND 65");

        let strict_res = convert_condition(&json!({
            "key": "age",
            "range": { "gt": 18, "lt": 65 }
        }))
        .expect("strict condition");
        assert_eq!(strict_res, "age > 18 AND age < 65");

        let mixed_res = convert_condition(&json!({
            "key": "age",
            "range": { "gte": 18, "lt": 65 }
        }))
        .expect("mixed condition");
        assert_eq!(mixed_res, "age >= 18 AND age < 65");
    }

    #[test]
    fn geo_predicates_error_with_predicate_name() {
        for pred in ["geo_bounding_box", "geo_radius", "geo_polygon"] {
            let err = convert_condition(&json!({"key": "loc", pred: {}})).unwrap_err();
            assert_eq!(err, ConvertError::GeoUnsupported(pred));
        }
    }
}
