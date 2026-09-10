use std::collections::HashMap;

use serde::de::Error as DeError;
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

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
    /// Typed payload (search hits, count, facet entries, or a raw envelope),
    /// when the operation returns data.
    pub data: Option<ExecData>,
    /// Server telemetry (`time` + hardware/inference `usage`) when the
    /// backend reported it. `None` where the route/transport carries none
    /// (batch items, collection DDL over gRPC, mocks without timing).
    /// `skip_serializing_if` keeps the pre-telemetry JSON shape byte-identical
    /// when no telemetry was reported; `default` reads old payloads back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<ServerTelemetry>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Point ID (integer or string/UUID).
    pub id: PlanPointId,
    /// Similarity or rerank score (defaults to 0.0 for unscored retrieved points).
    ///
    /// Serialized as the shortest f32 round-trip decimal (`0.95`, not
    /// `0.949999988079071`): Qdrant's own JSON text and the native Python
    /// `ScoredPoint.score` getter both use that form, so the JSON report view
    /// and the typed view agree.
    #[serde(default, serialize_with = "serialize_score_f32")]
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

/// Serialize an f32 with its shortest round-trip decimal (`0.95`), matching
/// Qdrant's JSON text and the Python `ScoredPoint.score` getter, instead of
/// serde_json's default f64 widening (`0.949999988079071`).
fn serialize_score_f32<S>(score: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match score.to_string().parse::<f64>() {
        Ok(value) => serializer.serialize_f64(value),
        Err(_) => serializer.serialize_f32(*score),
    }
}

/// Grouped query result: one group key with its ordered hits.
///
/// Serializes to Qdrant's group shape `{"id": …, "hits": […]}` so report JSON
/// and SDK `.groups()` consumers read the same field names as the backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupedSearchResult {
    /// Group key value as returned by Qdrant (JSON-typed).
    #[serde(rename = "id")]
    pub group_id: serde_json::Value,
    /// Search hits in this group, in backend order.
    pub hits: Vec<SearchHit>,
}

/// One facet entry: value + occurrence count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FacetHit {
    /// Facet value as returned by Qdrant (JSON-typed).
    pub value: serde_json::Value,
    /// Number of points carrying this value.
    pub count: u64,
}

/// Transport-neutral typed payload of one operation result.
///
/// Serialization emits the report JSON shapes SDK consumers already read:
/// `Hits` → array of [`SearchHit`], `Groups` → `{"groups": [{"id", "hits"}]}`,
/// `Count` → `{"count": n}`, `Facet` → array of `{"value": …, "count": n}`,
/// `Mutation { affected: Some(n) }` → `{"count": n}`,
/// `Mutation { affected: None }` → `null`, `Raw` → the envelope verbatim.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecData {
    /// Query / scroll / retrieve hits, in backend order.
    Hits(Vec<SearchHit>),
    /// Grouped query results (`QUERY … GROUP BY`).
    Groups(Vec<GroupedSearchResult>),
    /// COUNT result.
    Count(u64),
    /// FACET entries.
    Facet(Vec<FacetHit>),
    /// Write-op outcome: `affected: Some(n)` for upsert (n points written),
    /// `None` for status-only mutations (delete, payload/vector updates).
    Mutation {
        /// Number of points written, when the operation reports one.
        affected: Option<u64>,
    },
    /// Schemaless DDL / metadata / quota envelopes not yet modelled.
    Raw(serde_json::Value),
}

impl Serialize for ExecData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ExecData::Hits(hits) => hits.serialize(serializer),
            ExecData::Groups(groups) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("groups", groups)?;
                map.end()
            }
            ExecData::Count(count) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("count", count)?;
                map.end()
            }
            ExecData::Facet(hits) => hits.serialize(serializer),
            ExecData::Mutation {
                affected: Some(count),
            } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("count", count)?;
                map.end()
            }
            ExecData::Mutation { affected: None } => serializer.serialize_none(),
            ExecData::Raw(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ExecData {
    /// Shape-detecting parse for tests, mocks, and stored reports: object with
    /// a `groups` array → [`ExecData::Groups`]; array of `{value, count}` →
    /// [`ExecData::Facet`]; array of objects containing `id` → [`ExecData::Hits`];
    /// object with numeric `count` and no `result` wrapper → [`ExecData::Count`];
    /// everything else → [`ExecData::Raw`]. An empty array is read as empty
    /// [`ExecData::Hits`] (the pre-typing `hits()` behavior).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value {
            serde_json::Value::Array(items) if items.is_empty() => ExecData::Hits(Vec::new()),
            serde_json::Value::Array(items) => {
                if items
                    .iter()
                    .all(|item| item.get("value").is_some() && item.get("count").is_some())
                {
                    let facet: Vec<FacetHit> =
                        serde_json::from_value(serde_json::Value::Array(items))
                            .map_err(DeError::custom)?;
                    ExecData::Facet(facet)
                } else if items.iter().all(|item| item.get("id").is_some()) {
                    let hits: Vec<SearchHit> =
                        serde_json::from_value(serde_json::Value::Array(items))
                            .map_err(DeError::custom)?;
                    ExecData::Hits(hits)
                } else {
                    ExecData::Raw(serde_json::Value::Array(items))
                }
            }
            serde_json::Value::Object(map)
                if map.get("groups").is_some_and(serde_json::Value::is_array) =>
            {
                let groups = map
                    .get("groups")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let groups: Vec<GroupedSearchResult> =
                    serde_json::from_value(groups).map_err(DeError::custom)?;
                ExecData::Groups(groups)
            }
            serde_json::Value::Object(map) => {
                match map.get("count").and_then(serde_json::Value::as_u64) {
                    Some(count) if !map.contains_key("result") => ExecData::Count(count),
                    _ => ExecData::Raw(serde_json::Value::Object(map)),
                }
            }
            other => ExecData::Raw(other),
        })
    }
}

impl ExecData {
    /// Borrow hits when this is a [`ExecData::Hits`] payload. A query that
    /// found nothing still yields `Some(&[])`, matching the pre-typing
    /// `hits()` contract.
    pub fn hits(&self) -> Option<&[SearchHit]> {
        match self {
            ExecData::Hits(hits) => Some(hits),
            _ => None,
        }
    }

    /// Borrow grouped results when this is an [`ExecData::Groups`] payload.
    pub fn groups(&self) -> Option<&[GroupedSearchResult]> {
        match self {
            ExecData::Groups(groups) => Some(groups),
            _ => None,
        }
    }

    /// The integer when this is a [`ExecData::Count`] payload, or the affected
    /// point count of an upsert [`ExecData::Mutation`].
    pub fn count(&self) -> Option<u64> {
        match self {
            ExecData::Count(count) => Some(*count),
            ExecData::Mutation { affected } => *affected,
            _ => None,
        }
    }

    /// The affected-point count of an [`ExecData::Mutation`]: `Some(Some(n))`
    /// for an upsert, `Some(None)` for a status-only write, `None` otherwise.
    pub fn mutation_affected(&self) -> Option<Option<u64>> {
        match self {
            ExecData::Mutation { affected } => Some(*affected),
            _ => None,
        }
    }

    /// Borrow facet entries when this is a [`ExecData::Facet`] payload.
    pub fn facet(&self) -> Option<&[FacetHit]> {
        match self {
            ExecData::Facet(hits) => Some(hits),
            _ => None,
        }
    }

    /// Borrow the raw envelope when this payload is not yet modelled
    /// (DDL / metadata / quota responses).
    pub fn as_raw(&self) -> Option<&serde_json::Value> {
        match self {
            ExecData::Raw(value) => Some(value),
            _ => None,
        }
    }

    /// Whether the payload carries no records: empty `Hits`/`Groups`/`Facet`,
    /// or a `Raw` null / empty array / empty object. `Count` and `Mutation`
    /// are never empty (zero is a result).
    pub fn is_empty(&self) -> bool {
        match self {
            ExecData::Hits(hits) => hits.is_empty(),
            ExecData::Groups(groups) => groups.is_empty(),
            ExecData::Count(_) => false,
            ExecData::Facet(hits) => hits.is_empty(),
            ExecData::Mutation { .. } => false,
            ExecData::Raw(value) => match value {
                serde_json::Value::Null => true,
                serde_json::Value::Array(items) => items.is_empty(),
                serde_json::Value::Object(map) => map.is_empty(),
                _ => false,
            },
        }
    }
}

/// Typed backend response: data + telemetry, no transport JSON envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendResponse {
    /// Typed operation payload.
    pub data: ExecData,
    /// Server telemetry extracted from the transport envelope, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<ServerTelemetry>,
}

impl ExecResponse {
    /// Borrow the typed search hits if this response contains hits data.
    pub fn hits_ref(&self) -> Option<&[SearchHit]> {
        self.data.as_ref().and_then(ExecData::hits)
    }

    /// Typed search hits: cloned from the native typed payload. Non-hits
    /// payloads (counts, facets, status envelopes) yield `None`.
    pub fn hits(&self) -> Option<Vec<SearchHit>> {
        self.hits_ref().map(<[SearchHit]>::to_vec)
    }

    /// Return point IDs as u64 from hits or scroll results.
    pub fn ids(&self) -> Vec<u64> {
        let Some(hits) = self.hits_ref() else {
            return Vec::new();
        };
        hits.iter()
            .filter_map(|hit| match &hit.id {
                PlanPointId::Number(id) => Some(*id),
                PlanPointId::String(id) => id.parse().ok(),
            })
            .collect()
    }

    /// Return the count from a COUNT response, if present.
    pub fn count(&self) -> Option<u64> {
        self.data.as_ref().and_then(ExecData::count)
    }

    /// Return facet entries as `(value, count)` pairs, if present.
    pub fn facet(&self) -> Option<Vec<(serde_json::Value, u64)>> {
        let entries = self.data.as_ref()?.facet()?;
        Some(
            entries
                .iter()
                .map(|entry| (entry.value.clone(), entry.count))
                .collect(),
        )
    }
}
