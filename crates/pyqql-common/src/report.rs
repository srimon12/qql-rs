//! Native Python result classes shared by `pyqql` and `pyqql-edge`.
//!
//! [`PyExecutionReport`] and [`PyScoredPoint`] wrap the typed
//! [`qql::executor::ExecutionReport`] / [`SearchHit`] values directly: hits
//! are read from [`ExecData::Hits`] without a JSON round-trip, and only the
//! lazily accessed `results` / `telemetry` / payload views go through
//! `pythonize`. Both SDKs register the classes via
//! [`register_report_classes`].

use pyo3::exceptions::PyKeyError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql::executor::{ExecData, ExecutionReport, SearchHit};
use qql_plan::PlanPointId;

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

    /// Extracted payload text, or `None`.
    #[getter]
    fn text<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.inner.text {
            Some(text) => Ok(text.as_str().into_pyobject(py)?.into_any()),
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
            self.inner.text,
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
    /// Hydrate a report from the serialized dict shape (`{ok, results,
    /// succeeded, failed, telemetry?}`) for mocks, tests, and replay. Live
    /// results come from a client's `execute()`.
    #[new]
    #[pyo3(signature = (data=None))]
    fn new(data: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let inner = match data {
            Some(value) if !value.is_none() => pythonize::depythonize(value)?,
            _ => ExecutionReport::from_results(Vec::new()),
        };
        Ok(Self { inner })
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

    /// `GROUP BY` groups for statement `stmt` as raw backend group objects
    /// (`{id, hits}`), normalized across the `{"result": {"groups": […]}}`
    /// and bare `{"groups": […]}` envelopes.
    #[pyo3(signature = (stmt=0))]
    fn groups<'py>(&self, py: Python<'py>, stmt: isize) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        if let Some(result) = self.result_at(stmt)
            && let Some(data) = result.data.as_ref()
        {
            let value = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
            let groups = value
                .get("result")
                .and_then(|result| result.get("groups"))
                .or_else(|| value.get("groups"));
            if let Some(groups) = groups.and_then(serde_json::Value::as_array) {
                for group in groups {
                    out.append(pythonize::pythonize(py, group)?)?;
                }
            }
        }
        Ok(out)
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
