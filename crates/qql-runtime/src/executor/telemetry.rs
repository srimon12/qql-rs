//! Phase-1 telemetry: client-side phase timings plus server `time`/`usage`
//! extraction.
//!
//! Two halves, kept separate on purpose:
//!
//! - [`PhaseTimings`] — client stopwatches (`std::time::Instant`, no new
//!   dependencies; `qql-runtime` has no `tracing` infra to reuse) around the
//!   existing execute path. Millisecond floats.
//! - [`ServerTelemetry`] — Qdrant's `time` (server seconds, float) plus the
//!   hardware/inference `usage` object, extracted leniently from REST JSON
//!   envelopes (`{result, status, time, usage}` per `openapi.json`) and from
//!   the gRPC REST-shaped envelopes (`grpc_route` passes `usage` through in
//!   the same shape). Anything absent or misshapen becomes `None` — missing
//!   telemetry never fails a query.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Qdrant hardware counters (`HardwareUsage`: seven uint fields).
///
/// Field-level `#[serde(default)]` keeps parsing total: a counter the server
/// omits reads as zero rather than voiding the whole struct. A structurally
/// wrong `hardware` value (non-object, float counters) still voids just this
/// section — see [`ServerTelemetry::from_envelope`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareUsage {
    /// Raw CPU cycles (or abstract units) spent processing the request.
    #[serde(default)]
    pub cpu: u64,
    /// Payload bytes read from disk.
    #[serde(default)]
    pub payload_io_read: u64,
    /// Payload bytes written to disk.
    #[serde(default)]
    pub payload_io_write: u64,
    /// Payload-index bytes read from disk.
    #[serde(default)]
    pub payload_index_io_read: u64,
    /// Payload-index bytes written to disk.
    #[serde(default)]
    pub payload_index_io_write: u64,
    /// Vector bytes read from disk.
    #[serde(default)]
    pub vector_io_read: u64,
    /// Vector bytes written to disk.
    #[serde(default)]
    pub vector_io_write: u64,
}

impl HardwareUsage {
    /// Element-wise sum (saturating) for report-level totals.
    pub fn saturating_add(&self, other: &Self) -> Self {
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
pub struct ModelUsage {
    /// Tokens processed for this model.
    #[serde(default)]
    pub tokens: u64,
}

/// Inference usage: token spend keyed by model name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceUsage {
    /// Token spend per model (`models` map; empty when the server sent none).
    #[serde(default)]
    pub models: HashMap<String, ModelUsage>,
}

/// The `usage` half of server telemetry: optional hardware counters plus
/// optional per-model inference spend.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerUsage {
    /// Hardware counters, when the backend reported them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<HardwareUsage>,
    /// Per-model inference spend, when the backend reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<InferenceUsage>,
}

impl ServerUsage {
    /// Lenient parse of a REST `usage` value (`null` / missing / misshapen →
    /// `None`). Each section parses independently: a bad `hardware` section
    /// does not void a good `inference` section.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
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
    pub fn merge(&self, other: &Self) -> Self {
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
                        .and_modify(|e: &mut ModelUsage| {
                            e.tokens = e.tokens.saturating_add(usage.tokens);
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
/// and transports that do not report them (batch items, collection DDL over
/// gRPC, mocks) yield `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerTelemetry {
    /// Seconds the server spent processing the request, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_s: Option<f64>,
    /// Hardware/inference usage, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ServerUsage>,
}

impl ServerTelemetry {
    /// Extract from a backend response envelope. Lenient by contract: absent
    /// or misshapen `time`/`usage` becomes `None`, never an error.
    pub fn from_envelope(value: &serde_json::Value) -> Self {
        Self {
            time_s: value.get("time").and_then(|t| t.as_f64()),
            usage: value.get("usage").and_then(ServerUsage::from_json),
        }
    }

    /// `None` when the envelope carried neither half (keeps
    /// `ExecResponse.telemetry` exactly `None` where the backend sent
    /// nothing); `Some` otherwise, even if only one half is present.
    pub fn from_envelope_opt(value: &serde_json::Value) -> Option<Self> {
        let tel = Self::from_envelope(value);
        if tel.time_s.is_none() && tel.usage.is_none() {
            None
        } else {
            Some(tel)
        }
    }

    /// Aggregate per-response telemetry into report-level totals: server
    /// times summed (absent when no response reported one), usage merged.
    /// `None` when there is nothing to aggregate.
    pub fn aggregate<'a>(
        items: impl Iterator<Item = &'a ServerTelemetry>,
    ) -> Option<ServerTelemetry> {
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
        Some(ServerTelemetry { time_s, usage })
    }
}

/// Per-phase client timings for one analyzed statement, in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PhaseTimings {
    /// Source parse (`Parser::parse`; `0.0` for pre-parsed `Stmt` input).
    pub parse_ms: f64,
    /// `prepare_statement` (schema `USING` resolution + embedding resolution)
    /// plus `plan()`.
    pub prepare_plan_ms: f64,
    /// Backend round-trip (`QdrantOps::execute_planned`, incl. batch fan-out
    /// where the executor batches).
    pub dispatch_ms: f64,
    /// Response decode + normalization into `ExecResponse`.
    pub decode_ms: f64,
    /// Wall-clock total for the analyzed statement.
    pub total_ms: f64,
}
