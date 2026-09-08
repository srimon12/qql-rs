//! QQL happy path (Rust) — offline, no Qdrant server.
//!
//! Runs in CI: parse → hybrid CTE as text → tenant isolation → bind → the
//! `:rows` ingest shape. Live execution (`Executor::execute`,
//! `Executor::upsert_many`) needs a server and is covered by the vs-qdrant
//! harness, not here.

use qql_core::ast::{ComparisonOp, Value};
use qql_core::params::bind_stmt;
use qql_core::parser::Parser;
use std::collections::HashMap;

fn main() {
    // 1. One language for complex retrieval — hybrid fusion as text.
    let q = "
WITH
  dense  AS (QUERY TEXT 'vector databases' FROM docs USING dense  LIMIT 100),
  sparse AS (QUERY TEXT 'vector databases' FROM docs USING sparse LIMIT 100)
QUERY FUSION RRF FROM docs PREFETCH (dense, sparse) LIMIT 10
";
    let mut stmt = Parser::parse(q).expect("valid QQL");

    // 2. Tenant isolation: inject_filter always; SHARD routes.
    qql_core::ast::inject_filter(
        &mut stmt,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();

    // 3. Bind a vector param offline.
    let mut param_stmt =
        Parser::parse("QUERY :v FROM docs WHERE tenant_id = :t LIMIT :lim").unwrap();
    let mut params = HashMap::new();
    params.insert(
        "v".to_string(),
        Value::List(vec![
            Value::Float(0.1),
            Value::Float(0.2),
            Value::Float(0.3),
        ]),
    );
    params.insert("t".to_string(), Value::Str("acme".into()));
    params.insert("lim".to_string(), Value::Int(10));
    bind_stmt(&mut param_stmt, |k| params.get(k).cloned(), &[]).unwrap();

    // 4. Ingest shape is data, not text: point dicts splice into `:rows`.
    // Live: `exec.upsert_many("docs", rows, 100, OnError::Stop).await`.
    let mut tpl = Parser::parse("UPSERT INTO docs VALUES :rows").unwrap();
    let rows = Value::List(vec![
        Value::Dict(vec![
            ("id".into(), Value::Int(1)),
            (
                "vector".into(),
                Value::Dict(vec![(
                    "dense".into(),
                    Value::List(vec![Value::Float(0.1), Value::Float(0.2)]),
                )]),
            ),
        ]),
        Value::Dict(vec![("id".into(), Value::Int(2))]),
    ]);
    bind_stmt(
        &mut tpl,
        |k| (k == "rows").then(|| rows.clone()),
        &[],
    )
    .unwrap();
    let rendered = qql_core::fmt::format_stmt(&tpl);
    assert!(rendered.contains("id: 1") && rendered.contains("id: 2"));

    println!("quickstart ok: hybrid + isolation + bind + :rows");
}
