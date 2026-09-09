//! Client-side telemetry for the WASM transport: server `time` and `usage`
//! extraction plus report-level aggregation.
//!
//! Mirrors `qql-runtime` telemetry leniently: absent or misshapen halves
//! become `None`, never an error. Shapes match the Rust `ServerTelemetry`
//! JSON (`{"time_s": ..., "usage": {"hardware": ..., "inference": ...}}`)
//! so `dx.js`, Python, and Node read the same contract.

/// Extract server telemetry from a Qdrant REST envelope.
///
/// Reads `time` (seconds, float) and `usage` (object) leniently. Returns
/// `None` when the envelope carries neither half.
pub(crate) fn telemetry_from_envelope(envelope: &serde_json::Value) -> Option<serde_json::Value> {
    let time_s = envelope.get("time").and_then(|t| t.as_f64());
    let usage = envelope
        .get("usage")
        .and_then(|u| if u.is_object() { Some(u.clone()) } else { None });
    match (time_s, usage) {
        (None, None) => None,
        (time, use_) => {
            let mut obj = serde_json::Map::new();
            if let Some(t) = time {
                obj.insert("time_s".to_string(), serde_json::Value::from(t));
            }
            if let Some(u) = use_ {
                // Keep only the known sections; a misshapen section is dropped
                // while a good one survives.
                if let Some(filtered) = filter_usage(&u) {
                    obj.insert("usage".to_string(), filtered);
                }
            }
            if obj.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(obj))
            }
        }
    }
}

/// Keep the `hardware` and `inference` sections when they parse; drop a bad
/// section while keeping a good one. Returns `None` when neither survives.
fn filter_usage(usage: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = usage.as_object()?;
    let mut out = serde_json::Map::new();
    if let Some(h) = obj.get("hardware")
        && h.is_object()
    {
        out.insert("hardware".to_string(), h.clone());
    }
    if let Some(i) = obj.get("inference")
        && i.is_object()
    {
        out.insert("inference".to_string(), i.clone());
    }
    if out.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(out))
    }
}

/// Aggregate per-response telemetry into report-level totals: server times
/// summed, hardware counters summed, per-model tokens summed. `None` when
/// there is nothing to aggregate.
pub(crate) fn aggregate_telemetry(results: &[serde_json::Value]) -> Option<serde_json::Value> {
    let mut time_sum: Option<f64> = None;
    let mut usage_acc: Option<serde_json::Value> = None;
    let mut any = false;
    for res in results {
        let Some(tel) = res.get("telemetry") else {
            continue;
        };
        let has_time = tel.get("time_s").and_then(|t| t.as_f64()).is_some();
        let has_usage = tel.get("usage").is_some();
        if !has_time && !has_usage {
            continue;
        }
        any = true;
        if let Some(t) = tel.get("time_s").and_then(|t| t.as_f64()) {
            time_sum = Some(time_sum.unwrap_or(0.0) + t);
        }
        if let Some(u) = tel.get("usage") {
            usage_acc = Some(match usage_acc {
                Some(acc) => merge_usage(&acc, u),
                None => u.clone(),
            });
        }
    }
    if !any {
        return None;
    }
    let mut obj = serde_json::Map::new();
    if let Some(t) = time_sum {
        obj.insert("time_s".to_string(), serde_json::Value::from(t));
    }
    if let Some(u) = usage_acc {
        obj.insert("usage".to_string(), u);
    }
    if obj.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(obj))
    }
}

/// Merge two `usage` objects: hardware counters summed, per-model tokens summed.
fn merge_usage(a: &serde_json::Value, b: &serde_json::Value) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    let a_obj = a.as_object();
    let b_obj = b.as_object();
    if let Some(h) = merge_hardware(
        a_obj.and_then(|o| o.get("hardware")),
        b_obj.and_then(|o| o.get("hardware")),
    ) {
        out.insert("hardware".to_string(), h);
    }
    if let Some(i) = merge_inference(
        a_obj.and_then(|o| o.get("inference")),
        b_obj.and_then(|o| o.get("inference")),
    ) {
        out.insert("inference".to_string(), i);
    }
    serde_json::Value::Object(out)
}

fn merge_hardware(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    const FIELDS: [&str; 7] = [
        "cpu",
        "payload_io_read",
        "payload_io_write",
        "payload_index_io_read",
        "payload_index_io_write",
        "vector_io_read",
        "vector_io_write",
    ];
    let (Some(a), Some(b)) = (a, b) else {
        return a.cloned().or_else(|| b.cloned());
    };
    let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) else {
        return a
            .as_object()
            .map(|_| a.clone())
            .or_else(|| b.clone().into());
    };
    let mut out = serde_json::Map::new();
    for field in FIELDS {
        let av = ao.get(field).and_then(|v| v.as_u64()).unwrap_or(0);
        let bv = bo.get(field).and_then(|v| v.as_u64()).unwrap_or(0);
        out.insert(
            field.to_string(),
            serde_json::Value::from(av.saturating_add(bv)),
        );
    }
    Some(serde_json::Value::Object(out))
}

fn merge_inference(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    let (Some(a), Some(b)) = (a, b) else {
        return a.cloned().or_else(|| b.cloned());
    };
    let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) else {
        return a
            .as_object()
            .map(|_| a.clone())
            .or_else(|| b.clone().into());
    };
    let empty = serde_json::Map::new();
    let a_models = ao
        .get("models")
        .and_then(|m| m.as_object())
        .unwrap_or(&empty);
    let b_models = bo
        .get("models")
        .and_then(|m| m.as_object())
        .unwrap_or(&empty);
    let mut models = serde_json::Map::new();
    for (name, val) in a_models.iter().chain(b_models.iter()) {
        let tokens = val.get("tokens").and_then(|t| t.as_u64()).unwrap_or(0);
        let entry = models
            .entry(name.clone())
            .or_insert_with(|| serde_json::json!({"tokens": 0}));
        let current = entry.get("tokens").and_then(|t| t.as_u64()).unwrap_or(0);
        *entry = serde_json::json!({"tokens": current.saturating_add(tokens)});
    }
    let mut out = serde_json::Map::new();
    out.insert("models".to_string(), serde_json::Value::Object(models));
    Some(serde_json::Value::Object(out))
}
