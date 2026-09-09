//! `EXPLAIN ANALYZE`: static plan summary plus measured execution.
//!
//! Single-statement only (Postgres semantics): `Parser::parse` already fails
//! closed on multi-statement scripts, so scripts surface as parse errors
//! rather than silently analyzing just the first statement. Every phase
//! reuses the existing path — `Executor::prepare_statement`, `plan()`,
//! `Executor::dispatch_raw`/`Executor::normalize_planned` — timed with
//! client stopwatches around them; nothing here forks execution logic.

use std::collections::HashMap;
use std::time::Instant;

use qql_core::ast::{Stmt, Value};
use qql_core::error::QqlError;
use qql_core::parser;
use qql_plan::plan;

use crate::executor::telemetry::PhaseTimings;
use crate::executor::{AnalyzeReport, ExecResponse, Executor, OnError};

/// Elapsed milliseconds since `start`.
fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

impl Executor {
    /// Analyze one QQL statement: static plan plus per-phase client timings,
    /// server time, and hardware/inference usage.
    ///
    /// `on_error` is accepted for call-site uniformity with [`Executor::execute`];
    /// a single analyzed statement has no batch-continuation semantics, so it
    /// has no effect.
    pub async fn explain_analyze(
        &self,
        query: &str,
        on_error: OnError,
    ) -> Result<AnalyzeReport, QqlError> {
        self.ensure_open()?;
        let _ = on_error;
        let start = Instant::now();
        // `parse_all` + arity check (rather than `Parser::parse`) so a
        // multi-statement script fails closed with an explicit validation
        // error instead of a trailing-tokens parse error.
        let mut statements = parser::Parser::parse_all(query)?;
        let stmt = if statements.len() == 1 {
            statements.pop().expect("single stmt")
        } else if statements.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to analyze; the query string is empty or contains only comments",
                None,
            ));
        } else {
            return Err(QqlError::validation(
                "QQL-VALIDATION-MULTI-STMT",
                "explain_analyze accepts a single statement; analyze each statement separately",
                None,
            ));
        };
        let parse_ms = elapsed_ms(start);
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.analyze_stmt(stmt, parse_ms),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("operation timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.analyze_stmt(stmt, parse_ms).await
        }
    }

    /// Analyze one parameterized statement with named parameters (`:name`),
    /// mirroring [`Executor::execute_with_named_params`].
    pub async fn explain_analyze_with_named_params<K: AsRef<str>>(
        &self,
        query: &str,
        params: &[(K, Value)],
        on_error: OnError,
    ) -> Result<AnalyzeReport, QqlError> {
        self.ensure_open()?;
        let _ = on_error;
        let start = Instant::now();
        let mut statements = parser::Parser::parse_all(query)?;
        let mut stmt = if statements.len() == 1 {
            statements.pop().expect("single stmt")
        } else if statements.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to analyze; the query string is empty or contains only comments",
                None,
            ));
        } else {
            return Err(QqlError::validation(
                "QQL-VALIDATION-MULTI-STMT",
                "explain_analyze accepts a single statement; analyze each statement separately",
                None,
            ));
        };
        qql_core::params::bind_stmt(
            &mut stmt,
            |k| {
                params
                    .iter()
                    .find(|(name, _)| name.as_ref() == k)
                    .map(|(_, v)| v.clone())
            },
            &[],
        )?;
        let parse_ms = elapsed_ms(start);
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.analyze_stmt(stmt, parse_ms),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("operation timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.analyze_stmt(stmt, parse_ms).await
        }
    }

    /// Analyze one parameterized statement with named parameters given as a
    /// `HashMap`, mirroring [`Executor::execute_with_params`].
    pub async fn explain_analyze_with_params(
        &self,
        query: &str,
        params: &HashMap<String, Value>,
        on_error: OnError,
    ) -> Result<AnalyzeReport, QqlError> {
        self.ensure_open()?;
        let _ = on_error;
        let start = Instant::now();
        let mut statements = parser::Parser::parse_all(query)?;
        let mut stmt = if statements.len() == 1 {
            statements.pop().expect("single stmt")
        } else if statements.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to analyze; the query string is empty or contains only comments",
                None,
            ));
        } else {
            return Err(QqlError::validation(
                "QQL-VALIDATION-MULTI-STMT",
                "explain_analyze accepts a single statement; analyze each statement separately",
                None,
            ));
        };
        qql_core::params::bind_stmt(&mut stmt, |k| params.get(k).cloned(), &[])?;
        let parse_ms = elapsed_ms(start);
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.analyze_stmt(stmt, parse_ms),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("operation timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.analyze_stmt(stmt, parse_ms).await
        }
    }

    /// Analyze an already-parsed statement (no parse phase; `parse_ms` is
    /// `0.0`). Used by bindings that normalize `Stmt` inputs.
    pub async fn explain_analyze_node(
        &self,
        stmt: Stmt,
        on_error: OnError,
    ) -> Result<AnalyzeReport, QqlError> {
        self.ensure_open()?;
        let _ = on_error;
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.analyze_stmt(stmt, 0.0),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("operation timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.analyze_stmt(stmt, 0.0).await
        }
    }

    /// Shared analyze body: static plan first, then the timed
    /// prepare → plan → dispatch → normalize path.
    async fn analyze_stmt(&self, stmt: Stmt, parse_ms: f64) -> Result<AnalyzeReport, QqlError> {
        self.ensure_open()?;
        let total = Instant::now();
        // Static plan of the (bound) statement, before preparation mutates or
        // resolves anything — the same tree `explain` renders.
        let plan_text = qql_core::explain::explain_node(&stmt);

        let start = Instant::now();
        let prepared = self.prepare_statement(stmt).await?;
        let planned = plan(&prepared)?;
        let prepare_plan_ms = elapsed_ms(start);

        // Client-side pair scorer: never a single Qdrant route, so there is
        // no dispatch/decode split — the whole client-side run is dispatch.
        if let qql_plan::PlannedOperation::CrossRerank { .. } = planned {
            let start = Instant::now();
            let resp = self.dispatch_planned(&planned).await?;
            let dispatch_ms = elapsed_ms(start);
            return Ok(AnalyzeReport {
                ok: true,
                plan: plan_text,
                phases: PhaseTimings {
                    parse_ms,
                    prepare_plan_ms,
                    dispatch_ms,
                    decode_ms: 0.0,
                    total_ms: elapsed_ms(total),
                },
                server_time_s: resp.telemetry.as_ref().and_then(|t| t.time_s),
                usage: resp.telemetry.as_ref().and_then(|t| t.usage.clone()),
                results: vec![resp],
            });
        }

        let start = Instant::now();
        let raw = self.dispatch_raw(&planned).await?;
        let dispatch_ms = elapsed_ms(start);

        let start = Instant::now();
        let resp: ExecResponse = Self::normalize_planned(&planned, raw)?;
        let decode_ms = elapsed_ms(start);

        Ok(AnalyzeReport {
            ok: true,
            plan: plan_text,
            phases: PhaseTimings {
                parse_ms,
                prepare_plan_ms,
                dispatch_ms,
                decode_ms,
                total_ms: elapsed_ms(total),
            },
            server_time_s: resp.telemetry.as_ref().and_then(|t| t.time_s),
            usage: resp.telemetry.as_ref().and_then(|t| t.usage.clone()),
            results: vec![resp],
        })
    }
}
