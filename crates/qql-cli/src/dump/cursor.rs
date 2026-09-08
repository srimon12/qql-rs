//! Cursor extraction and point ID parsing for scroll pagination.

use qql_plan::semantic::PlanPointId;
use serde_json::Value;

/// Extract `(points, next_page_offset)` from a scroll response body.
pub fn extract_scroll_page(response: &Value) -> (Vec<Value>, Option<PlanPointId>) {
    let result = response.get("result").unwrap_or(response);

    let points = result
        .get("points")
        .and_then(|p| p.as_array())
        .cloned()
        .or_else(|| result.as_array().cloned())
        .unwrap_or_default();

    let next = result
        .get("next_page_offset")
        .and_then(json_to_plan_point_id);

    (points, next)
}

/// Convert a JSON number or string point identifier to typed `PlanPointId`.
pub fn json_to_plan_point_id(v: &Value) -> Option<PlanPointId> {
    match v {
        Value::Number(n) => n.as_u64().map(PlanPointId::Number),
        Value::String(s) => Some(PlanPointId::String(s.clone())),
        _ => None,
    }
}
