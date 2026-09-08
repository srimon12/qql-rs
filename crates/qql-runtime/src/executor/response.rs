use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use qql_core::error::QqlError;
use qql_plan::PlanPointId;

/// Single-statement execution outcome: status, operation label, message, and data.
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
}

impl ExecutionReport {
    /// Create from a collection of `ExecResponse`s. `ok` is `failed == 0`.
    pub fn from_results(results: Vec<ExecResponse>) -> Self {
        let succeeded = results.iter().filter(|r| r.ok).count();
        let failed = results.len() - succeeded;
        Self {
            ok: failed == 0,
            results,
            succeeded,
            failed,
        }
    }

    /// Convenience wrapper for a single `ExecResponse`.
    pub fn single(resp: ExecResponse) -> Self {
        let ok = resp.ok;
        Self {
            ok,
            results: vec![resp],
            succeeded: if ok { 1 } else { 0 },
            failed: if ok { 0 } else { 1 },
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
    /// Return search hits as a slice of JSON values if this response contains hits data.
    pub fn hits_json(&self) -> Option<&[serde_json::Value]> {
        self.data
            .as_ref()
            .and_then(|d| d.as_array().map(|v| v.as_slice()))
    }

    /// Deserialize hits into typed `Vec<SearchHit>` if present.
    pub fn hits(&self) -> Option<Vec<SearchHit>> {
        self.data
            .as_ref()
            .and_then(|d| serde_json::from_value(d.clone()).ok())
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
