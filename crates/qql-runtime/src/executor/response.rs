use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use qql_core::error::QqlError;
use qql_plan::PlanPointId;

use super::telemetry::{PhaseTimings, ServerTelemetry, ServerUsage};

/// Single-statement execution outcome: status, operation label, message, data,
/// and optional server telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    /// Whether the statement succeeded.
    pub ok: bool,
    /// Operation label (e.g. `QUERY`, `UPSERT`, `PARSE`) for this result.
    pub operation: String,
    /// Human-readable summary or error text.
    pub message: String,
    /// JSON payload (search hits, raw result, or counts), when the operation returns data.
    pub data: Option<serde_json::Value>,
    /// Server telemetry (`time` + hardware/inference `usage`) when the
    /// backend reported it. `None` where the route/transport carries none
    /// (batch items, collection DDL over gRPC, mocks without timing).
    /// `skip_serializing_if` keeps the pre-telemetry JSON shape byte-identical
    /// when no telemetry was reported; `default` reads old payloads back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<ServerTelemetry>,
    /// Client-side typed-hit cache, skipped on the wire. Normalization
    /// attaches the `Vec<SearchHit>` it already built (no re-parse), and
    /// [`ExecResponse::hits`] parses `data` at most once otherwise — Rust
    /// callers get typed access without a per-call JSON clone + parse while
    /// `hits_json()` keeps borrowing the raw payload. `#[serde(skip)]` keeps
    /// the JSON shape byte-identical; `OnceLock` is `Send + Sync` so
    /// responses stay shareable across threads.
    #[serde(skip)]
    pub typed_hits: std::sync::OnceLock<Option<Vec<SearchHit>>>,
}

/// Canonical cross-SDK execution result. Every `client.execute(…)` call
/// returns this shape regardless of input type (string / Stmt / array).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    /// Whether every statement succeeded (`failed == 0`).
    pub ok: bool,
    /// One `ExecResponse` per statement, in execution order.
    pub results: Vec<ExecResponse>,
    /// Number of successful statements.
    pub succeeded: usize,
    /// Number of failed statements.
    pub failed: usize,
    /// Report-level telemetry totals (server times summed, hardware counters
    /// summed, per-model tokens summed). `None` when no response reported
    /// telemetry. Same back-compat serde contract as
    /// [`ExecResponse::telemetry`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<ServerTelemetry>,
}

impl ExecutionReport {
    /// Create from a collection of `ExecResponse`s. `ok` is `failed == 0`;
    /// `telemetry` aggregates the per-response telemetry (see
    /// [`ServerTelemetry::aggregate`]).
    pub fn from_results(results: Vec<ExecResponse>) -> Self {
        let succeeded = results.iter().filter(|r| r.ok).count();
        let failed = results.len() - succeeded;
        let telemetry =
            ServerTelemetry::aggregate(results.iter().filter_map(|r| r.telemetry.as_ref()));
        Self {
            ok: failed == 0,
            results,
            succeeded,
            failed,
            telemetry,
        }
    }

    /// Convenience wrapper for a single `ExecResponse`.
    pub fn single(resp: ExecResponse) -> Self {
        let ok = resp.ok;
        let telemetry = resp.telemetry.clone();
        Self {
            ok,
            results: vec![resp],
            succeeded: if ok { 1 } else { 0 },
            failed: if ok { 0 } else { 1 },
            telemetry,
        }
    }

    /// Return the first statement response, if present.
    pub fn first(&self) -> Option<&ExecResponse> {
        self.results.first()
    }

    /// Return search hits of statement `stmt` as a JSON slice.
    pub fn hits_json(&self, stmt: usize) -> Option<&[serde_json::Value]> {
        self.results.get(stmt).and_then(|r| r.hits_json())
    }

    /// Return typed search hits of statement `stmt`.
    pub fn hits(&self, stmt: usize) -> Option<Vec<SearchHit>> {
        self.results.get(stmt).and_then(|r| r.hits())
    }

    /// Return point IDs from statement `stmt`.
    pub fn ids(&self, stmt: usize) -> Vec<u64> {
        self.results.get(stmt).map(|r| r.ids()).unwrap_or_default()
    }

    /// Return the count integer from statement `stmt`.
    pub fn count(&self, stmt: usize) -> Option<u64> {
        self.results.get(stmt).and_then(|r| r.count())
    }

    /// Return facet entries as `(value, count)` pairs from statement `stmt`.
    pub fn facet(&self, stmt: usize) -> Option<Vec<(serde_json::Value, u64)>> {
        self.results.get(stmt).and_then(|r| r.facet())
    }

    /// Return the count integer from the first statement.
    pub fn first_count(&self) -> Option<u64> {
        self.count(0)
    }

    /// Return the search hits from the first statement.
    pub fn first_hits(&self) -> Option<Vec<SearchHit>> {
        self.hits(0)
    }

    /// Return the search hits from the first statement as a JSON slice.
    pub fn first_hits_json(&self) -> Option<&[serde_json::Value]> {
        self.hits_json(0)
    }

    /// Return the search hits from the first statement as a raw vector of JSON objects.
    pub fn first_hits_raw(&self) -> Vec<serde_json::Value> {
        self.first_hits_json()
            .map(|s| s.to_vec())
            .unwrap_or_default()
    }

    /// Return the point IDs from the first statement.
    pub fn first_ids(&self) -> Vec<u64> {
        self.ids(0)
    }

    /// Return the facet pairs from the first statement.
    pub fn first_facet(&self) -> Option<Vec<(serde_json::Value, u64)>> {
        self.facet(0)
    }
}

/// Structured `EXPLAIN ANALYZE` report: the static plan summary plus the
/// measured execution. JSON-serializable; returned by
/// [`Executor::explain_analyze`](super::Executor::explain_analyze).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeReport {
    /// Whether the analyzed statement succeeded (errors surface as `Err`).
    pub ok: bool,
    /// Static plan summary — the same tree [`Executor::explain`](super::Executor::explain)
    /// renders, captured before execution.
    pub plan: String,
    /// Per-phase client timings (milliseconds).
    pub phases: PhaseTimings,
    /// Total server time in seconds (summed per-response `time`); `None`
    /// when the backend reported none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_time_s: Option<f64>,
    /// Merged hardware/inference usage; `None` when the backend reported none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ServerUsage>,
    /// The statement's execution response(s), in order (one entry: analyze
    /// is single-statement, like Postgres `EXPLAIN ANALYZE`).
    pub results: Vec<ExecResponse>,
}

/// Controls batch-execution behaviour when a statement fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// Halt immediately on the first error (default).
    #[default]
    Stop,
    /// Continue executing remaining statements, collecting error
    /// responses alongside successes.
    Continue,
}

/// Normalized search hit returned inside `ExecResponse` data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// Point ID (integer or string/UUID).
    pub id: PlanPointId,
    /// Similarity or rerank score (defaults to 0.0 for unscored retrieved points).
    #[serde(default)]
    pub score: f32,
    /// Payload text extracted for text-centric results, when present.
    pub text: Option<String>,
    /// Point payload when requested via `WITH PAYLOAD`.
    pub payload: Option<HashMap<String, serde_json::Value>>,
    /// Source collection. Populated by cross-collection operations (e.g.
    /// CROSS RERANK) so results are unambiguous when multiple collections
    /// share the same point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    /// Vector(s) returned when requested via `WITH VECTOR`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<serde_json::Value>,
}

/// Grouped query result: one group key with its ordered hits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedSearchResult {
    /// Group key value as returned by Qdrant (JSON-typed).
    pub group_id: serde_json::Value,
    /// Search hits in this group, in backend order.
    pub hits: Vec<SearchHit>,
}

impl ExecResponse {
    /// Attach an already-built typed hit list (normalization path): `hits()`
    /// serves these without touching the JSON `data` copy.
    pub(crate) fn with_typed_hits(self, hits: Vec<SearchHit>) -> Self {
        let _ = self.typed_hits.set(Some(hits));
        self
    }

    /// Return search hits as a slice of JSON values if this response contains hits data.
    pub fn hits_json(&self) -> Option<&[serde_json::Value]> {
        self.data
            .as_ref()
            .and_then(|d| d.as_array().map(|v| v.as_slice()))
    }

    /// Typed search hits: served from the normalization cache when present,
    /// parsed from `data` at most once otherwise (borrowed parse — the JSON
    /// payload is never cloned). Non-hits payloads (counts, facets, status
    /// envelopes) yield `None`, exactly as before.
    pub fn hits(&self) -> Option<Vec<SearchHit>> {
        self.typed_hits
            .get_or_init(|| {
                self.data.as_ref().and_then(|d| {
                    // Borrowed `&Value` deserialization: identical result to
                    // `from_value(d.clone())` with one fewer full-payload copy.
                    <Vec<SearchHit>>::deserialize(d).ok()
                })
            })
            .clone()
    }

    /// Return point IDs as u64 from hits or scroll results.
    pub fn ids(&self) -> Vec<u64> {
        let Some(arr) = self.hits_json() else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|h| {
                h.get("id").and_then(|id| {
                    id.as_u64()
                        .or_else(|| id.as_str().and_then(|s| s.parse().ok()))
                })
            })
            .collect()
    }

    /// Return the count from a COUNT response, if present.
    pub fn count(&self) -> Option<u64> {
        self.data.as_ref().and_then(|d| {
            d.get("result")
                .and_then(|r| r.get("count"))
                .or_else(|| d.get("count"))
                .and_then(|c| c.as_u64())
        })
    }

    /// Return facet entries as `(value, count)` pairs, if present.
    pub fn facet(&self) -> Option<Vec<(serde_json::Value, u64)>> {
        let arr = self.data.as_ref()?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let val = item.get("value")?.clone();
            let count = item.get("count")?.as_u64()?;
            out.push((val, count));
        }
        Some(out)
    }
}

/// Serialize a list of [`SearchHit`]s to a [`serde_json::Value::Array`].
///
/// Serialization cannot currently fail for [`SearchHit`]'s field types, but
/// failing loudly here beats silently emitting `null` data on a future type
/// change.
pub(crate) fn serialize_hits(hits: &[SearchHit]) -> Result<serde_json::Value, QqlError> {
    serde_json::to_value(hits).map_err(|error| {
        QqlError::execution(
            "QQL-RESPONSE-SERIALIZE",
            format!("failed to serialize search results: {error}"),
            None,
        )
    })
}
