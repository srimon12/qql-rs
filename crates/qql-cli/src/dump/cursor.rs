//! Cursor extraction and point ID parsing for scroll pagination.

use qql::executor::BackendResponse;
use qql_plan::semantic::PlanPointId;
use serde_json::Value;

/// Extract JSON points and the next-page cursor from a typed scroll response.
///
/// [`ExecData::Hits`](qql::executor::ExecData::Hits) does not carry Qdrant's
/// `next_page_offset`, so the cursor always comes from the last point id via
/// [`next_scroll_cursor`]; [`ScrollPages`](super::ScrollPages) then removes the
/// inclusive-offset repeat with [`drop_resumed_point`].
pub fn extract_scroll_page(response: &BackendResponse) -> (Vec<Value>, Option<PlanPointId>) {
    let points = response
        .data
        .hits()
        .map(|hits| {
            hits.iter()
                .filter_map(|hit| serde_json::to_value(hit).ok())
                .collect()
        })
        .unwrap_or_default();
    (points, None)
}

/// Convert a JSON number or string point identifier to typed `PlanPointId`.
pub fn json_to_plan_point_id(v: &Value) -> Option<PlanPointId> {
    match v {
        Value::Number(n) => n.as_u64().map(PlanPointId::Number),
        Value::String(s) => Some(PlanPointId::String(s.clone())),
        _ => None,
    }
}
