use std::collections::{BTreeSet, HashMap};

use qql_core::ast::{self, Stmt, Value};
use qql_core::error::QqlError;
use qql_core::parser;
use qql_plan::plan_template;

use crate::backend::CollectionInfo;
use crate::executor::{ExecResponse, ExecutionReport, Executor, OnError};

/// A parsed and pre-compiled query template for repeated execution with different parameters.
#[derive(Debug, Clone)]
pub struct PreparedStatement {
    pub(crate) sql: String,
    pub(crate) stmt: Stmt,
    pub(crate) planned: Option<qql_plan::PlannedOperation>,
    pub(crate) named_params: BTreeSet<String>,
    pub(crate) positional_count: usize,
    /// Whether the template carries whole-point placeholders (`VALUES :p`).
    pub(crate) has_point_params: bool,
    /// Collection schema fetched once at [`Executor::prepare`] for templates
    /// with point placeholders, reused by every point-splice execution
    /// instead of re-fetching per call.
    pub(crate) upsert_schema: Option<CollectionInfo>,
}

impl PreparedStatement {
    /// The original SQL query template string.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The parsed AST statement.
    pub fn stmt(&self) -> &Stmt {
        &self.stmt
    }

    /// Whether this prepared statement has a fast execution path: either a
    /// pre-compiled planned operation template or whole-point placeholders
    /// bound via the point-splice path.
    pub fn is_planned(&self) -> bool {
        self.planned.is_some() || self.has_point_params
    }

    /// Parameter names referenced in this statement template.
    pub fn named_params(&self) -> &BTreeSet<String> {
        &self.named_params
    }

    /// Total count of positional parameter placeholders (`?`) referenced in this statement template.
    ///
    /// Positional parameters use 0-based indexing: the first `?` corresponds to index 0
    /// (`params[0]`), the second to index 1 (`params[1]`), etc.
    pub fn positional_count(&self) -> usize {
        self.positional_count
    }
}

impl Executor {
    /// Prepare a query template for repeated execution with different parameters.
    pub async fn prepare(&self, sql: &str) -> Result<PreparedStatement, QqlError> {
        self.ensure_open()?;
        let mut stmts = parser::Parser::parse_all(sql)?;
        let stmt = if stmts.len() == 1 {
            stmts.pop().expect("single stmt")
        } else if stmts.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "cannot prepare an empty query template",
                None,
            ));
        } else {
            return Err(QqlError::validation(
                "QQL-VALIDATION-MULTI-STMT",
                "cannot prepare a multi-statement script; prepare one statement at a time",
                None,
            ));
        };
        self.prepare_from_stmt(sql.to_string(), stmt).await
    }

    /// Bulk ingest helper: the canonical fast path for application ingest.
    ///
    /// Prepares `UPSERT INTO <collection> VALUES :rows` once (schema fetched
    /// once), then splices each `batch_size` chunk of point dicts through the
    /// point-splice path. Each `rows` entry is one `{id, vector, …payload}`
    /// point dict — the same shape as inline `VALUES {…}` rows.
    ///
    /// The template is built as AST (never interpolated SQL), so collection
    /// names are data, not syntax. `batch_size == 0` fails closed;
    /// empty `rows` returns an empty `ok` report without I/O.
    pub async fn upsert_many(
        &self,
        collection: &str,
        rows: Vec<Value>,
        batch_size: usize,
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        if batch_size == 0 {
            return Err(QqlError::validation(
                "QQL-VALIDATION-UPSERT-BATCH",
                "upsert_many batch_size must be >= 1",
                None,
            ));
        }
        if rows.is_empty() {
            return Ok(ExecutionReport::from_results(Vec::new()));
        }
        let stmt = Stmt::Upsert(Box::new(qql_core::ast::UpsertStmt {
            collection: collection.to_string(),
            points: vec![ast::PointEntry::Param("rows".to_string(), None)],
            embedding: None,
            embed: Vec::new(),
            shard_key: None,
            wait: None,
        }));
        // `prepare` re-checks openness; the template carries no user SQL.
        let prepared = self
            .prepare_from_stmt(format!("UPSERT INTO {collection} VALUES :rows"), stmt)
            .await?;
        let stop_on_error = matches!(on_error, OnError::Stop);
        let mut results = Vec::new();
        // Move (never clone) each chunk out of `rows`: the point dicts are
        // already owned, and a deep clone here would duplicate every vector
        // element once more for no reason.
        let mut rows = rows.into_iter();
        loop {
            let chunk: Vec<Value> = rows.by_ref().take(batch_size).collect();
            if chunk.is_empty() {
                break;
            }
            let mut params = HashMap::with_capacity(1);
            params.insert("rows".to_string(), Value::List(chunk));
            match self.execute_prepared(&prepared, &params).await {
                Ok(resp) => results.push(resp),
                Err(e) if !stop_on_error => results.push(ExecResponse {
                    ok: false,
                    operation: "UPSERT".to_string(),
                    message: e.to_string(),
                    data: None,
                }),
                Err(e) => return Err(e),
            }
        }
        Ok(ExecutionReport::from_results(results))
    }

    /// Shared preparation body: schema resolution + template planning for an
    /// already-parsed statement. [`Executor::prepare`] parses first;
    /// [`Executor::upsert_many`] builds its template as AST (no SQL
    /// interpolation, so collection names stay data).
    pub(crate) async fn prepare_from_stmt(
        &self,
        sql: String,
        stmt: Stmt,
    ) -> Result<PreparedStatement, QqlError> {
        self.ensure_open()?;
        let (named_params, positional_count) = qql_core::params::collect_statement_params(&stmt);
        let has_point_params = qql_core::params::stmt_has_point_params(&stmt);
        let planned = match self.prepare_statement(stmt.clone()).await {
            Ok(prep) => plan_template(&prep).ok(),
            Err(_) => None,
        };
        // Point-placeholder templates resolve bound points against this
        // schema on every execution instead of re-fetching per call. Fetched
        // once here; `None` (missing collection) disables the fast path and
        // every execution takes the checking slow path instead.
        let upsert_schema = if has_point_params {
            if let Stmt::Upsert(upsert) = &stmt
                && self
                    .client
                    .collection_exists(&upsert.collection)
                    .await
                    .unwrap_or(false)
            {
                self.get_cached_collection_info(&upsert.collection)
                    .await
                    .ok()
            } else {
                None
            }
        } else {
            None
        };
        Ok(PreparedStatement {
            sql,
            stmt,
            planned,
            named_params,
            positional_count,
            has_point_params,
            upsert_schema,
        })
    }

    /// Execute a prepared statement with named parameters.
    pub async fn execute_prepared(
        &self,
        prepared: &PreparedStatement,
        params: &HashMap<String, Value>,
    ) -> Result<ExecResponse, QqlError> {
        self.ensure_open()?;
        if prepared.named_params.is_empty() && prepared.positional_count == 0 && !params.is_empty()
        {
            return Err(QqlError::validation(
                "QQL-BIND-UNUSED-PARAMS",
                "query has no parameter placeholders, but parameters were provided",
                None,
            ));
        }
        for k in params.keys() {
            if !prepared.named_params.contains(k) {
                return Err(QqlError::validation(
                    "QQL-BIND-UNUSED-PARAMS",
                    format!("parameter ':{k}' was provided but is not used in the query template"),
                    None,
                ));
            }
        }
        if prepared.has_point_params {
            for required in &prepared.named_params {
                if !params.contains_key(required) {
                    return Err(QqlError::validation(
                        "QQL-BIND-MISSING-PARAM",
                        format!("missing value for named parameter ':{required}'"),
                        None,
                    ));
                }
            }
            return self
                .execute_prepared_upsert_points(prepared, &|k| params.get(k).cloned(), &[])
                .await;
        }
        if let Some(ref template_op) = prepared.planned {
            for required in &prepared.named_params {
                let Some(val) = params.get(required) else {
                    return Err(QqlError::validation(
                        "QQL-BIND-MISSING-PARAM",
                        format!("missing value for named parameter ':{required}'"),
                        None,
                    ));
                };
                if qql_plan::PlanVectorValue::from_value(val).is_none() {
                    return Err(QqlError::validation(
                        "QQL-BIND-INVALID-PARAMS",
                        format!("parameter ':{required}' must be a vector (list of floats)"),
                        None,
                    ));
                }
            }
            let mut op = template_op.clone();
            op.bind_vector_params(
                &|name| {
                    params
                        .get(name)
                        .and_then(qql_plan::PlanVectorValue::from_value)
                },
                &|_| None,
            );
            return self.dispatch_planned(&op).await;
        }
        let mut stmt = prepared.stmt.clone();
        qql_core::params::bind_stmt(&mut stmt, |k| params.get(k).cloned(), &[])?;
        self.execute_node(stmt).await
    }

    /// Execute a prepared statement with positional parameters (`?`).
    ///
    /// Positional parameters use 0-based indexing: `params[0]` binds to the first `?`,
    /// `params[1]` binds to the second `?`, up to `prepared.positional_count() - 1`.
    ///
    /// Returns `QQL-BIND-UNUSED-PARAMS` if more parameters are supplied than `?` placeholders,
    /// or `QQL-BIND-MISSING-PARAM` if fewer parameters are supplied.
    pub async fn execute_prepared_positional(
        &self,
        prepared: &PreparedStatement,
        params: &[Value],
    ) -> Result<ExecResponse, QqlError> {
        self.ensure_open()?;
        if prepared.positional_count == 0 && prepared.named_params.is_empty() && !params.is_empty()
        {
            return Err(QqlError::validation(
                "QQL-BIND-UNUSED-PARAMS",
                "query has no parameter placeholders, but positional parameters were provided",
                None,
            ));
        }
        if params.len() > prepared.positional_count {
            return Err(QqlError::validation(
                "QQL-BIND-UNUSED-PARAMS",
                format!(
                    "too many positional parameters provided: expected {}, but {} were provided",
                    prepared.positional_count,
                    params.len()
                ),
                None,
            ));
        }
        if prepared.has_point_params {
            if params.len() < prepared.positional_count {
                return Err(QqlError::validation(
                    "QQL-BIND-MISSING-PARAM",
                    format!(
                        "missing value for positional parameter '?{}' (only {} parameter(s) provided)",
                        params.len() + 1,
                        params.len()
                    ),
                    None,
                ));
            }
            return self
                .execute_prepared_upsert_points(prepared, &|_| None, params)
                .await;
        }
        if let Some(ref template_op) = prepared.planned {
            if params.len() < prepared.positional_count {
                return Err(QqlError::validation(
                    "QQL-BIND-MISSING-PARAM",
                    format!(
                        "missing value for positional parameter '?{}' (only {} parameter(s) provided)",
                        params.len() + 1,
                        params.len()
                    ),
                    None,
                ));
            }
            for (idx, val) in params.iter().take(prepared.positional_count).enumerate() {
                if qql_plan::PlanVectorValue::from_value(val).is_none() {
                    return Err(QqlError::validation(
                        "QQL-BIND-INVALID-PARAMS",
                        format!(
                            "positional parameter '?{}' must be a vector (list of floats)",
                            idx + 1
                        ),
                        None,
                    ));
                }
            }
            let mut op = template_op.clone();
            op.bind_vector_params(&|_| None, &|idx| {
                params
                    .get(idx)
                    .and_then(qql_plan::PlanVectorValue::from_value)
            });
            return self.dispatch_planned(&op).await;
        }
        let mut stmt = prepared.stmt.clone();
        qql_core::params::bind_stmt(&mut stmt, |_| None, params)?;
        self.execute_node(stmt).await
    }

    /// Point-splice fast path for upsert templates with whole-point
    /// placeholders (`VALUES :p` / `VALUES ?`).
    async fn execute_prepared_upsert_points(
        &self,
        prepared: &PreparedStatement,
        lookup: &impl Fn(&str) -> Option<Value>,
        positional: &[Value],
    ) -> Result<ExecResponse, QqlError> {
        let mut stmt = prepared.stmt.clone();
        qql_core::params::bind_stmt(&mut stmt, lookup, positional)?;
        let Stmt::Upsert(mut upsert) = stmt else {
            return Err(QqlError::validation(
                "QQL-BIND-INVALID-PARAMS",
                "point parameters are only supported by UPSERT templates",
                None,
            ));
        };
        if self.embedder.is_some() || upsert.embedding.is_some() || !upsert.embed.is_empty() {
            return self.execute_node(Stmt::Upsert(upsert)).await;
        }
        let needs_slow_path = upsert.points.iter().any(|point| {
            !matches!(
                point,
                ast::PointEntry::Inline(inline) if inline.vectors.is_some()
            )
        });
        if needs_slow_path || prepared.upsert_schema.is_none() {
            return self.execute_node(Stmt::Upsert(upsert)).await;
        }
        if let Some(info) = prepared.upsert_schema.as_ref() {
            crate::executor::dml::upsert::map_unnamed_to_single_dense(&mut upsert, info);
            self.validate_embedded_upsert(&upsert, info)?;
        }
        let request = qql_plan::mutation::lower_upsert_request(&upsert);
        let wait = upsert
            .wait
            .unwrap_or(upsert.embedding.is_some() || !upsert.embed.is_empty());
        let op = qql_plan::PlannedOperation::Upsert {
            collection: upsert.collection.clone(),
            request,
            wait,
        };
        self.dispatch_planned(&op).await
    }
}
