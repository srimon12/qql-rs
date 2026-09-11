//! Client-side telemetry for the WASM transport: server `time` and `usage`
//! extraction plus report-level aggregation.
//!
//! Mirrors the runtime's typed [`ServerTelemetry`] contract field-for-field so
//! the JSON `dx.js`, Python, and Node read is identical. Extraction reads
//! exactly the envelope's `time` (seconds, float) and `usage` (object) keys;
//! absent or misshapen telemetry degrades to `None` and never fails a
//! successful response — telemetry is the one lenient extraction, matching
//! `qql-runtime`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Qdrant hardware counters (`HardwareUsage`: seven uint fields).
///
/// Field-level `#[serde(default)]` keeps parsing total: a counter the server
/// omits reads as zero. A structurally wrong `hardware` value (non-object,
/// float counters) voids just this section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HardwareUsage {
    #[serde(default)]
    pub(crate) cpu: u64,
    #[serde(default)]
    pub(crate) payload_io_read: u64,
    #[serde(default)]
    pub(crate) payload_io_write: u64,
    #[serde(default)]
    pub(crate) payload_index_io_read: u64,
    #[serde(default)]
    pub(crate) payload_index_io_write: u64,
    #[serde(default)]
    pub(crate) vector_io_read: u64,
    #[serde(default)]
    pub(crate) vector_io_write: u64,
}

impl HardwareUsage {
    /// Element-wise sum (saturating) for report-level totals.
    fn saturating_add(&self, other: &Self) -> Self {
        Self {
            cpu: self.cpu.saturating_add(other.cpu),
            payload_io_read: self.payload_io_read.saturating_add(other.payload_io_read),
            payload_io_write: self.payload_io_write.saturating_add(other.payload_io_write),
            payload_index_io_read: self
                .payload_index_io_read
                .saturating_add(other.payload_index_io_read),
            payload_index_io_write: self
                .payload_index_io_write
                .saturating_add(other.payload_index_io_write),
            vector_io_read: self.vector_io_read.saturating_add(other.vector_io_read),
            vector_io_write: self.vector_io_write.saturating_add(other.vector_io_write),
        }
    }
}

/// Per-model inference token spend (`ModelUsage{tokens}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ModelUsage {
    #[serde(default)]
    pub(crate) tokens: u64,
}

/// Inference usage: token spend keyed by model name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InferenceUsage {
    /// Token spend per model (`models` map; empty when the server sent none).
    #[serde(default)]
    pub(crate) models: BTreeMap<String, ModelUsage>,
}

/// The `usage` half of server telemetry: optional hardware counters plus
/// optional per-model inference spend.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) hardware: Option<HardwareUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) inference: Option<InferenceUsage>,
}

impl ServerUsage {
    /// Lenient parse of a REST `usage` value (`null` / missing / misshapen →
    /// `None`). Each section parses independently: a bad `hardware` section
    /// does not void a good `inference` section.
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object()?;
        let hardware = obj
            .get("hardware")
            .and_then(|h| serde_json::from_value(h.clone()).ok());
        let inference = obj
            .get("inference")
            .and_then(|i| serde_json::from_value(i.clone()).ok());
        if hardware.is_none() && inference.is_none() {
            return None;
        }
        Some(Self {
            hardware,
            inference,
        })
    }

    /// Merge two usage reports for report-level totals: counters summed
    /// (saturating), per-model tokens summed.
    fn merge(&self, other: &Self) -> Self {
        let hardware = match (&self.hardware, &other.hardware) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };
        let inference = match (&self.inference, &other.inference) {
            (Some(a), Some(b)) => {
                let mut models = a.models.clone();
                for (name, usage) in &b.models {
                    models
                        .entry(name.clone())
                        .and_modify(|entry| {
                            entry.tokens = entry.tokens.saturating_add(usage.tokens);
                        })
                        .or_insert_with(|| usage.clone());
                }
                Some(InferenceUsage { models })
            }
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };
        Self {
            hardware,
            inference,
        }
    }
}

/// Server telemetry for one response: Qdrant's `time` (seconds, float) plus
/// the hardware/inference `usage` object. Both halves are optional — routes
/// that do not report them yield `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ServerTelemetry {
    /// Seconds the server spent processing the request, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) time_s: Option<f64>,
    /// Hardware/inference usage, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) usage: Option<ServerUsage>,
}

impl ServerTelemetry {
    /// Extract from a backend response envelope. Lenient by contract: absent
    /// or misshapen `time`/`usage` becomes `None`, never an error.
    pub(crate) fn from_envelope(value: &serde_json::Value) -> Self {
        Self {
            time_s: value.get("time").and_then(|t| t.as_f64()),
            usage: value.get("usage").and_then(ServerUsage::from_json),
        }
    }

    /// `None` when the envelope carried neither half (keeps per-response
    /// telemetry exactly `None` where the backend sent nothing); `Some`
    /// otherwise, even if only one half is present.
    fn from_envelope_opt(value: &serde_json::Value) -> Option<Self> {
        let telemetry = Self::from_envelope(value);
        if telemetry.time_s.is_none() && telemetry.usage.is_none() {
            None
        } else {
            Some(telemetry)
        }
    }

    /// Aggregate per-response telemetry into report-level totals: server
    /// times summed (absent when no response reported one), usage merged.
    /// `None` when there is nothing to aggregate.
    fn aggregate<'a>(items: impl Iterator<Item = &'a ServerTelemetry>) -> Option<Self> {
        let mut time_s: Option<f64> = None;
        let mut usage: Option<ServerUsage> = None;
        let mut any = false;
        for item in items {
            any = true;
            if let Some(t) = item.time_s {
                time_s = Some(time_s.unwrap_or(0.0) + t);
            }
            if let Some(u) = &item.usage {
                usage = Some(match usage {
                    Some(acc) => acc.merge(u),
                    None => u.clone(),
                });
            }
        }
        if !any || (time_s.is_none() && usage.is_none()) {
            return None;
        }
        Some(Self { time_s, usage })
    }
}

/// Extract the optional telemetry half of a REST envelope.
pub(crate) fn telemetry_from_envelope(envelope: &serde_json::Value) -> Option<ServerTelemetry> {
    ServerTelemetry::from_envelope_opt(envelope)
}

/// Aggregate the per-response `telemetry` values of a report's results.
/// Entries without telemetry (or with `null`) are skipped.
pub(crate) fn aggregate_telemetry(results: &[serde_json::Value]) -> Option<ServerTelemetry> {
    let parsed: Vec<ServerTelemetry> = results
        .iter()
        .filter_map(|result| result.get("telemetry"))
        .filter(|telemetry| !telemetry.is_null())
        .filter_map(|telemetry| serde_json::from_value(telemetry.clone()).ok())
        .collect();
    ServerTelemetry::aggregate(parsed.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_time_and_usage_strictly() {
        let telemetry = telemetry_from_envelope(&json!({
            "result": {},
            "time": 0.125,
            "usage": {
                "hardware": {"cpu": 7, "vector_io_read": 3},
                "inference": {"models": {"bge": {"tokens": 42}}},
            },
        }))
        .expect("telemetry");
        assert_eq!(telemetry.time_s, Some(0.125));
        // Typed serialization drops keys the runtime's types do not model.
        let value = serde_json::to_value(&telemetry).unwrap();
        assert_eq!(value["time_s"], 0.125);
        assert_eq!(value["usage"]["hardware"]["cpu"], 7);
        let usage = telemetry.usage.expect("usage");
        let hardware = usage.hardware.expect("hardware");
        assert_eq!(hardware.cpu, 7);
        assert_eq!(hardware.vector_io_read, 3);
        // Omitted counters default to zero, like the runtime.
        assert_eq!(hardware.payload_io_write, 0);
        let inference = usage.inference.expect("inference");
        assert_eq!(inference.models["bge"].tokens, 42);
    }

    #[test]
    fn absent_or_misshapen_telemetry_is_none() {
        assert!(telemetry_from_envelope(&json!({"result": {}})).is_none());
        assert!(telemetry_from_envelope(&json!({"time": "soon"})).is_none());
        // A malformed `usage` section voids only itself.
        let telemetry = telemetry_from_envelope(&json!({
            "time": 1.5,
            "usage": {"hardware": {"cpu": 1.5}, "inference": {"models": {}}},
        }))
        .expect("time survives");
        assert_eq!(telemetry.time_s, Some(1.5));
        let usage = telemetry.usage.expect("inference survives");
        assert!(usage.hardware.is_none());
        assert_eq!(usage.inference.unwrap().models.len(), 0);
    }

    #[test]
    fn aggregates_results_into_report_totals() {
        let results = vec![
            json!({
                "telemetry": {
                    "time_s": 1.0,
                    "usage": {
                        "hardware": {"cpu": 2, "vector_io_read": 5},
                        "inference": {"models": {"a": {"tokens": 3}}},
                    },
                },
            }),
            json!({"telemetry": null}),
            json!({}),
            json!({
                "telemetry": {
                    "time_s": 2.5,
                    "usage": {
                        "hardware": {"cpu": 4, "vector_io_read": 1},
                        "inference": {"models": {"a": {"tokens": 4}, "b": {"tokens": 9}}},
                    },
                },
            }),
        ];
        let aggregated = aggregate_telemetry(&results).expect("aggregated");
        assert_eq!(aggregated.time_s, Some(3.5));
        let usage = aggregated.usage.expect("usage");
        let hardware = usage.hardware.expect("hardware");
        assert_eq!(hardware.cpu, 6);
        assert_eq!(hardware.vector_io_read, 6);
        let models = usage.inference.expect("models").models;
        assert_eq!(models["a"].tokens, 7);
        assert_eq!(models["b"].tokens, 9);
        assert!(aggregate_telemetry(&[json!({})]).is_none());
    }
}
