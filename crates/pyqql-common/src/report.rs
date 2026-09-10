//! Native Python result classes shared by `pyqql` and `pyqql-edge`.
//!
//! [`PyExecutionReport`] and [`PyScoredPoint`] wrap the typed
//! [`qql::executor::ExecutionReport`] / [`SearchHit`] values directly: hits
//! are read from [`ExecData::Hits`] without a JSON round-trip, and only the
//! lazily accessed `results` / `telemetry` / payload views go through
//! `pythonize`. Both SDKs register the classes via
//! [`register_report_classes`].

use std::collections::HashMap;

use pyo3::exceptions::{PyKeyError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyType};
use qql::executor::{ExecData, ExecutionReport, FacetHit, GroupedSearchResult, SearchHit};
use qql_plan::{PlanFacetValue, PlanGroupId, PlanPointId, PlanVectorStruct};

/// Convert a typed point id to its Python form (`int` or `str`).
fn point_id_object<'py>(py: Python<'py>, id: &PlanPointId) -> PyResult<Bound<'py, PyAny>> {
    match id {
        PlanPointId::Number(n) => Ok(n.into_pyobject(py)?.into_any()),
        PlanPointId::String(s) => Ok(s.as_str().into_pyobject(py)?.into_any()),
    }
}

/// f32 → Python float via the shortest round-trip decimal, matching the
/// JSON/pythonize score rendering the SDKs used before native classes
/// (`0.95f32` becomes `0.95`, not `0.949999988079071`).
fn score_to_f64(score: f32) -> f64 {
    score.to_string().parse().unwrap_or(score as f64)
}

/// Extracted text is the payload's `text` field (the typed hit has no
/// dedicated field). Non-string payload `text` values yield `None`.
fn payload_text(payload: &Option<HashMap<String, serde_json::Value>>) -> Option<&str> {
    payload.as_ref()?.get("text")?.as_str()
}

/// A scored hit returned from a search or retrieval query.
#[pyclass(name = "ScoredPoint", frozen, skip_from_py_object)]
#[derive(Clone)]
pub struct PyScoredPoint {
    inner: SearchHit,
}

impl PyScoredPoint {
    /// Wrap a typed hit. Used by [`PyExecutionReport::hits`].
    fn new(inner: SearchHit) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyScoredPoint {
    /// Point id (`int` or `str`).
    #[getter]
    fn id<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        point_id_object(py, &self.inner.id)
    }

    /// Similarity or rerank score.
    #[getter]
    fn score(&self) -> f64 {
        score_to_f64(self.inner.score)
    }

    /// Point payload, or `None` when not requested.
    #[getter]
    fn payload<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.inner.payload {
            Some(payload) => Ok(pythonize::pythonize(py, payload)?),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Extracted payload text (`payload["text"]` when it is a string), or
    /// `None`.
    #[getter]
    fn text<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match payload_text(&self.inner.payload) {
            Some(text) => Ok(text.into_pyobject(py)?.into_any()),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Source collection for cross-collection operations, or `None`.
    #[getter]
    fn collection<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.inner.collection {
            Some(collection) => Ok(collection.as_str().into_pyobject(py)?.into_any()),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Vector(s) returned via `WITH VECTOR`, or `None`.
    #[getter]
    fn vector<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.inner.vector {
            Some(vector) => Ok(pythonize::pythonize(py, vector)?),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Field access: attribute fields first, then payload keys.
    fn __getitem__<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Bound<'py, PyAny>> {
        match key {
            "id" => self.id(py),
            "score" => Ok(self.score().into_pyobject(py)?.into_any()),
            "payload" => self.payload(py),
            "text" => self.text(py),
            "collection" => self.collection(py),
            "vector" => self.vector(py),
            _ => self.payload_lookup(py, key),
        }
    }

    /// `dict`-style `get`: attribute fields (falling back to `default` when
    /// unset), then payload keys, then `default`.
    #[pyo3(signature = (key, default=None))]
    fn get<'py>(
        &self,
        py: Python<'py>,
        key: &str,
        default: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let default = default.unwrap_or_else(|| py.None().into_bound(py));
        match key {
            "id" | "score" => self.__getitem__(py, key),
            "payload" | "text" | "collection" | "vector" => {
                let value = self.__getitem__(py, key)?;
                if value.is_none() {
                    Ok(default)
                } else {
                    Ok(value)
                }
            }
            _ => match self.payload_lookup(py, key) {
                Ok(value) => Ok(value),
                Err(_) => Ok(default),
            },
        }
    }

    /// Return a copy with `payload` stripped (`scroll_cursor(with_payload=False)`).
    fn without_payload(&self) -> PyScoredPoint {
        let mut inner = self.inner.clone();
        inner.payload = None;
        PyScoredPoint { inner }
    }

    fn __repr__(&self) -> String {
        format!(
            "ScoredPoint(id={}, score={}, text={:?}, collection={:?})",
            self.inner.id,
            score_to_f64(self.inner.score),
            payload_text(&self.inner.payload),
            self.inner.collection
        )
    }
}

impl PyScoredPoint {
    /// Payload lookup for `__getitem__` / `get`; `KeyError` when absent.
    fn payload_lookup<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Bound<'py, PyAny>> {
        let value = self
            .inner
            .payload
            .as_ref()
            .and_then(|payload| payload.get(key));
        match value {
            Some(value) => Ok(pythonize::pythonize(py, value)?),
            None => Err(PyKeyError::new_err(key.to_string())),
        }
    }
}

/// Execution report returned by `Client.execute()`.
///
/// Wraps the typed runtime report: `.hits()` reads the native
/// [`ExecData::Hits`] payload directly, while `.results` and `.telemetry`
/// serialize on access. `report["ok"]`, `report.count(0)`, and
/// `report.hits()` keep working.
#[pyclass(name = "ExecutionReport", frozen)]
pub struct PyExecutionReport {
    inner: ExecutionReport,
}

impl PyExecutionReport {
    /// Wrap a typed runtime [`ExecutionReport`] as the native Python
    /// `ExecutionReport` class.
    pub fn wrap<'py>(
        py: Python<'py>,
        report: ExecutionReport,
    ) -> PyResult<Bound<'py, PyExecutionReport>> {
        Bound::new(py, Self { inner: report })
    }

    /// Resolve a Python-style (negative-capable) statement index.
    fn result_at(&self, stmt: isize) -> Option<&qql::executor::ExecResponse> {
        let len = self.inner.results.len() as isize;
        let idx = if stmt < 0 { stmt + len } else { stmt };
        if idx < 0 || idx >= len {
            None
        } else {
            Some(&self.inner.results[idx as usize])
        }
    }
}

#[pymethods]
impl PyExecutionReport {
    /// Build an in-memory report from typed statement specs (tests, mocks,
    /// offline replay). Each spec is a dict with an `operation` string,
    /// an optional `ok` (default `True`) and `message`, and at most one typed
    /// payload key:
    ///
    /// - `hits`: list of `{id, score?, payload?, collection?, vector?}`
    /// - `groups`: list of `{id, hits}`
    /// - `count`: int — a COUNT result, or a mutation's affected count on any
    ///   other operation
    /// - `facet`: list of `{value, count}`
    /// - `collections`: list of collection names
    /// - `collection`: collection-info dict (`status`, `points_count`, …)
    /// - `shard_keys`: list of strings or non-negative ints
    /// - `quotas`: quota-config dict (`enabled`, `max_disk_usage_percent`, …)
    ///
    /// Live reports always come from a client's `execute()`; this constructor
    /// exists so offline tests can exercise the typed accessors without a
    /// serialized-envelope round trip.
    #[classmethod]
    #[pyo3(signature = (results))]
    fn from_results(_cls: &Bound<'_, PyType>, results: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut responses = Vec::new();
        for spec in results.try_iter()? {
            let spec = spec?;
            let spec = spec
                .cast::<PyDict>()
                .map_err(|_| PyTypeError::new_err("each result spec must be a dict"))?;
            responses.push(response_from_spec(spec)?);
        }
        Ok(Self {
            inner: ExecutionReport::from_results(responses),
        })
    }

    /// Whether every statement succeeded (`failed == 0`).
    #[getter]
    fn ok(&self) -> bool {
        self.inner.ok
    }

    /// Number of successful statements.
    #[getter]
    fn succeeded(&self) -> usize {
        self.inner.succeeded
    }

    /// Number of failed statements.
    #[getter]
    fn failed(&self) -> usize {
        self.inner.failed
    }

    /// Aggregated server telemetry (`time_s` plus hardware and inference
    /// `usage`) as a dict, or `None` when the backend reported none.
    #[getter]
    fn telemetry<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.inner.telemetry {
            Some(telemetry) => Ok(pythonize::pythonize(py, telemetry)?),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// One serialized dict (`{ok, operation, message, data?, telemetry?}`)
    /// per statement, in execution order.
    #[getter]
    fn results<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        for result in &self.inner.results {
            out.append(pythonize::pythonize(py, result)?)?;
        }
        Ok(out)
    }

    /// Typed `ScoredPoint` hits for statement `stmt` (default first).
    #[pyo3(signature = (stmt=0))]
    fn hits<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(hits) = result.hits_ref()
        {
            for hit in hits {
                out.append(Bound::new(py, PyScoredPoint::new(hit.clone()))?)?;
            }
        }
        Ok(out)
    }

    /// Alias for [`hits`](Self::hits).
    #[pyo3(signature = (stmt=0))]
    fn points<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        self.hits(py, stmt)
    }

    /// Point ids (`int` or `str`) for statement `stmt`.
    #[pyo3(signature = (stmt=0))]
    fn ids<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(hits) = result.hits_ref()
        {
            for hit in hits {
                out.append(point_id_object(py, &hit.id)?)?;
            }
        }
        Ok(out)
    }

    /// Facet entries (`[{value, count}, …]`) for statement `stmt`.
    #[pyo3(signature = (stmt=0))]
    fn facet<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(entries) = result.data.as_ref().and_then(ExecData::facet)
        {
            for entry in entries {
                let item = PyDict::new(py);
                item.set_item("value", pythonize::pythonize(py, &entry.value)?)?;
                item.set_item("count", entry.count)?;
                out.append(item)?;
            }
        }
        Ok(out)
    }

    /// Count integer for statement `stmt`; `0` when the statement carries no
    /// count.
    #[pyo3(signature = (stmt=0))]
    fn count(&self, stmt: isize) -> u64 {
        self.result_at(stmt)
            .and_then(qql::executor::ExecResponse::count)
            .unwrap_or(0)
    }

    /// `GROUP BY` groups for statement `stmt`: `[{id, hits: [ScoredPoint, …]}]`.
    #[pyo3(signature = (stmt=0))]
    fn groups<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(groups) = result.data.as_ref().and_then(ExecData::groups)
        {
            for group in groups {
                let item = PyDict::new(py);
                item.set_item("id", pythonize::pythonize(py, &group.group_id)?)?;
                let hits = PyList::empty(py);
                for hit in &group.hits {
                    hits.append(Bound::new(py, PyScoredPoint::new(hit.clone()))?)?;
                }
                item.set_item("hits", hits)?;
                out.append(item)?;
            }
        }
        Ok(out)
    }

    /// `SHOW COLLECTIONS` names for statement `stmt`.
    #[pyo3(signature = (stmt=0))]
    fn collections<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(collections) = result.data.as_ref().and_then(ExecData::collections)
        {
            for name in collections {
                out.append(name)?;
            }
        }
        Ok(out)
    }

    /// `SHOW COLLECTION` metadata as a dict, or `None`.
    #[pyo3(signature = (stmt=0))]
    fn collection<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyAny>> {
        match self
            .result_at(stmt)
            .and_then(|result| result.data.as_ref().and_then(ExecData::collection))
        {
            Some(info) => Ok(pythonize::pythonize(py, info)?),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// `SHOW SHARD KEYS` keys (`str` or `int`) for statement `stmt`.
    #[pyo3(signature = (stmt=0))]
    fn shard_keys<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(keys) = result.data.as_ref().and_then(ExecData::shard_keys)
        {
            for key in keys {
                out.append(pythonize::pythonize(py, key)?)?;
            }
        }
        Ok(out)
    }

    /// `SHOW QUOTAS` configuration as a dict, or `None`.
    #[pyo3(signature = (stmt=0))]
    fn quotas<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyAny>> {
        match self
            .result_at(stmt)
            .and_then(|result| result.data.as_ref().and_then(ExecData::quotas))
        {
            Some(config) => Ok(pythonize::pythonize(py, config)?),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Dict-style field access for `ok` / `results` / `succeeded` / `failed`
    /// / `telemetry`.
    fn __getitem__<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Bound<'py, PyAny>> {
        match key {
            "ok" => Ok(self.inner.ok.into_pyobject(py)?.to_owned().into_any()),
            "results" => Ok(self.results(py)?.into_any()),
            "succeeded" => Ok(self.inner.succeeded.into_pyobject(py)?.into_any()),
            "failed" => Ok(self.inner.failed.into_pyobject(py)?.into_any()),
            "telemetry" => self.telemetry(py),
            _ => Err(PyKeyError::new_err(key.to_string())),
        }
    }

    /// `dict`-style `get` over the same keys as
    /// [`__getitem__`](Self::__getitem__).
    #[pyo3(signature = (key, default=None))]
    fn get<'py>(
        &self,
        py: Python<'py>,
        key: &str,
        default: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let default = default.unwrap_or_else(|| py.None().into_bound(py));
        match key {
            "ok" | "results" | "succeeded" | "failed" | "telemetry" => self.__getitem__(py, key),
            _ => Ok(default),
        }
    }

    /// `dict`-style membership for the report fields.
    fn __contains__(&self, key: &str) -> bool {
        matches!(key, "ok" | "results" | "succeeded" | "failed" | "telemetry")
    }

    fn __repr__(&self) -> String {
        format!(
            "ExecutionReport(ok={}, succeeded={}, failed={}, results={})",
            self.inner.ok,
            self.inner.succeeded,
            self.inner.failed,
            self.inner.results.len()
        )
    }
}

/// Register the native `ScoredPoint` / `ExecutionReport` classes on a module.
/// Call once from each SDK's `#[pymodule]` init.
pub fn register_report_classes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyScoredPoint>()?;
    module.add_class::<PyExecutionReport>()?;
    Ok(())
}

// ── typed report construction from Python specs ──────────────────

/// Required spec field, failing with `KeyError` when absent.
fn required<'py>(dict: &Bound<'py, PyDict>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    dict.get_item(key)?
        .ok_or_else(|| PyKeyError::new_err(key.to_string()))
}

/// Optional non-`None` spec field.
fn optional<'py>(dict: &Bound<'py, PyDict>, key: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
    match dict.get_item(key)? {
        Some(value) if !value.is_none() => Ok(Some(value)),
        _ => Ok(None),
    }
}

/// Deserialize a typed field through the pythonize boundary (payload values,
/// vector/group/facet primitives, quotas, collection info).
fn typed_field<T: serde::de::DeserializeOwned>(
    value: &Bound<'_, PyAny>,
    what: &str,
) -> PyResult<T> {
    pythonize::depythonize(value)
        .map_err(|error| PyTypeError::new_err(format!("invalid {what}: {error}")))
}

/// One typed [`SearchHit`] from `{id, score?, payload?, collection?, vector?}`.
fn hit_from_python(value: &Bound<'_, PyAny>) -> PyResult<SearchHit> {
    let hit = value
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("each hit must be a dict"))?;
    let id: PlanPointId = typed_field(&required(hit, "id")?, "hit id")?;
    let score: f64 = optional(hit, "score")?
        .map(|value| value.extract())
        .transpose()?
        .unwrap_or(0.0);
    let payload: Option<HashMap<String, serde_json::Value>> = optional(hit, "payload")?
        .map(|value| typed_field(&value, "hit payload"))
        .transpose()?;
    let collection: Option<String> = optional(hit, "collection")?
        .map(|value| value.extract())
        .transpose()?;
    let vector: Option<PlanVectorStruct> = optional(hit, "vector")?
        .map(|value| typed_field(&value, "hit vector"))
        .transpose()?;
    Ok(SearchHit {
        id,
        score: score as f32,
        payload,
        collection,
        vector,
    })
}

/// Typed hits from a Python iterable of hit dicts.
fn hits_from_python(value: &Bound<'_, PyAny>) -> PyResult<Vec<SearchHit>> {
    value
        .try_iter()?
        .map(|item| item.and_then(|item| hit_from_python(&item)))
        .collect()
}

/// Typed groups from a Python iterable of `{id, hits}` dicts.
fn groups_from_python(value: &Bound<'_, PyAny>) -> PyResult<Vec<GroupedSearchResult>> {
    let mut groups = Vec::new();
    for item in value.try_iter()? {
        let item = item?;
        let group = item
            .cast::<PyDict>()
            .map_err(|_| PyTypeError::new_err("each group must be a dict"))?;
        let group_id: PlanGroupId = typed_field(&required(group, "id")?, "group id")?;
        let hits = match optional(group, "hits")? {
            Some(hits) => hits_from_python(&hits)?,
            None => Vec::new(),
        };
        groups.push(GroupedSearchResult { group_id, hits });
    }
    Ok(groups)
}

/// Typed facet entries from a Python iterable of `{value, count}` dicts.
fn facet_from_python(value: &Bound<'_, PyAny>) -> PyResult<Vec<FacetHit>> {
    let mut entries = Vec::new();
    for item in value.try_iter()? {
        let item = item?;
        let entry = item
            .cast::<PyDict>()
            .map_err(|_| PyTypeError::new_err("each facet entry must be a dict"))?;
        let facet_value: PlanFacetValue = typed_field(&required(entry, "value")?, "facet value")?;
        let count: u64 = required(entry, "count")?.extract()?;
        entries.push(FacetHit {
            value: facet_value,
            count,
        });
    }
    Ok(entries)
}

/// Typed payload from the spec's single variant key.
fn data_from_spec(spec: &Bound<'_, PyDict>, operation: &str) -> PyResult<Option<ExecData>> {
    if let Some(value) = optional(spec, "hits")? {
        return Ok(Some(ExecData::Hits(hits_from_python(&value)?)));
    }
    if let Some(value) = optional(spec, "groups")? {
        return Ok(Some(ExecData::Groups(groups_from_python(&value)?)));
    }
    if let Some(value) = optional(spec, "count")? {
        let count: u64 = value.extract()?;
        // `COUNT` produces a count result; every other operation reporting a
        // count is a mutation (UPSERT affected-points).
        return Ok(Some(if operation == "COUNT" {
            ExecData::Count(count)
        } else {
            ExecData::Mutation {
                affected: Some(count),
            }
        }));
    }
    if let Some(value) = optional(spec, "facet")? {
        return Ok(Some(ExecData::Facet(facet_from_python(&value)?)));
    }
    if let Some(value) = optional(spec, "collections")? {
        return Ok(Some(ExecData::Collections(value.extract()?)));
    }
    if let Some(value) = optional(spec, "collection")? {
        return Ok(Some(ExecData::Collection(typed_field(
            &value,
            "collection info",
        )?)));
    }
    if let Some(value) = optional(spec, "shard_keys")? {
        return Ok(Some(ExecData::ShardKeys(typed_field(
            &value,
            "shard keys",
        )?)));
    }
    if let Some(value) = optional(spec, "quotas")? {
        return Ok(Some(ExecData::Quotas(typed_field(&value, "quotas")?)));
    }
    Ok(None)
}

/// One typed [`ExecResponse`](qql::executor::ExecResponse) from a spec dict.
fn response_from_spec(spec: &Bound<'_, PyDict>) -> PyResult<qql::executor::ExecResponse> {
    let operation: String = required(spec, "operation")?.extract()?;
    let ok: bool = optional(spec, "ok")?
        .map(|value| value.extract())
        .transpose()?
        .unwrap_or(true);
    let message: String = optional(spec, "message")?
        .map(|value| value.extract())
        .transpose()?
        .unwrap_or_else(|| {
            if ok {
                format!("{operation} ok")
            } else {
                format!("{operation} failed")
            }
        });
    let data = if ok {
        data_from_spec(spec, &operation)?
    } else {
        None
    };
    Ok(qql::executor::ExecResponse {
        ok,
        operation,
        message,
        data,
        telemetry: None,
    })
}
