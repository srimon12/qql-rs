//! 0.3.2 hot paths: parameter binding, plan/compile gates, formatting.
//!
//! Covers the prepared-statement surface the 9-query corpus cannot: string
//! binding (`bind_str_with_params`), AST binding (`bind_stmt_with_params`),
//! the plan gate (`plan`), the `is_valid` gate (`parse_and_plan`), route
//! compilation (`compile_statement`), and canonical formatting (`format_stmt`).
//! Inputs come from `bench/queries.json` (`Bound` + `Full` entries).
//!
//! usage: `bench_bind [--iterations N] [--reps N] [--filter SUBSTR] [--json]`

#[path = "../common.rs"]
mod common;

use common::{Row, median, parse_args, print_header, print_json, print_row, run_reps};
use qql_core::fmt::format_stmt;
use qql_core::params_json::{bind_stmt_with_params, bind_str_with_params};
use qql_core::parser::Parser;
use qql_plan::{compile_statement, parse_and_plan, plan};

/// A named timed closure. Borrows its query strings from the caller.
type Case<'a> = (&'static str, Box<dyn FnMut() + 'a>);

fn corpus_entry(name: &str) -> (String, Option<serde_json::Value>) {
    let queries = common::load_queries();
    queries
        .into_iter()
        .find(|q| q.name == name)
        .map(|q| (q.qql, q.params))
        .unwrap_or_else(|| panic!("bench/queries.json must contain a '{name}' entry"))
}

fn main() {
    let args = parse_args("bench_bind", 50_000, 5);
    let (bound_qql, bound_params) = corpus_entry("Bound");
    let bound_params = bound_params.expect("Bound entry must carry params");
    let (full_qql, _) = corpus_entry("Full");
    let filter = args.filter.as_deref().map(str::to_ascii_lowercase);

    // (row name, closure) pairs so --filter applies to operations too.
    let mut cases: Vec<Case<'_>> = Vec::new();
    cases.push((
        "bind_str",
        Box::new(|| {
            bind_str_with_params(&bound_qql, &bound_params, false).expect("bind_str must succeed");
        }),
    ));
    cases.push((
        "parse+bind_stmt",
        Box::new(|| {
            let mut stmt = Parser::parse(&bound_qql).expect("Bound must parse");
            bind_stmt_with_params(&mut stmt, &bound_params).expect("bind_stmt must succeed");
        }),
    ));
    cases.push((
        "parse+bind+plan",
        Box::new(|| {
            let mut stmt = Parser::parse(&bound_qql).expect("Bound must parse");
            bind_stmt_with_params(&mut stmt, &bound_params).expect("bind_stmt must succeed");
            plan(&stmt).expect("bound stmt must plan");
        }),
    ));
    cases.push((
        "parse_and_plan",
        Box::new(|| {
            parse_and_plan(&full_qql).expect("Full must parse_and_plan");
        }),
    ));
    cases.push((
        "compile_statement",
        Box::new(|| {
            let stmt = Parser::parse(&full_qql).expect("Full must parse");
            compile_statement(&stmt).expect("Full must compile");
        }),
    ));
    cases.push((
        "format",
        Box::new(|| {
            let stmt = Parser::parse(&full_qql).expect("Full must parse");
            format_stmt(&stmt);
        }),
    ));
    if let Some(needle) = &filter {
        cases.retain(|(name, _)| name.to_ascii_lowercase().contains(needle));
        if cases.is_empty() {
            eprintln!("no bind cases match filter '{needle}'");
            std::process::exit(2);
        }
    }

    if !args.json {
        print_header("Rust qql 0.3.2 bind/compile paths", &args);
    }
    let mut rows = Vec::with_capacity(cases.len());
    for (name, mut work) in cases {
        let samples = run_reps(1_000, args.iterations, args.reps, &mut work);
        rows.push(Row {
            name: name.to_string(),
            ns_per_op: median(samples),
        });
        if !args.json {
            print_row(rows.last().expect("row just pushed"));
        }
    }
    if args.json {
        print_json("bench_bind", &args, &rows);
    }
}
