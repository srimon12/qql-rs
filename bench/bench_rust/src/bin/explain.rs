//! Pure sync compile throughput (`Executor::explain`) over `bench/queries.json`.
//!
//! Explanation rendering only — no backend, no embeddings.
//!
//! usage: `explain [--iterations N] [--reps N] [--filter SUBSTR] [--json]`

#[path = "../common.rs"]
mod common;

use common::{
    Row, apply_filter, load_queries, median, parse_args, print_header, print_json, print_row,
    run_reps,
};
use qql::executor::Executor;

fn main() {
    let args = parse_args("explain", 100_000, 5);
    let queries = apply_filter(load_queries(), args.filter.as_deref());

    if !args.json {
        print_header("Rust qql-runtime Pure Sync Compile (explain)", &args);
    }
    let mut rows = Vec::with_capacity(queries.len());
    for query in &queries {
        let samples = run_reps(100, args.iterations, args.reps, || {
            Executor::explain(&query.qql).expect("benchmark query must explain")
        });
        rows.push(Row {
            name: query.name.clone(),
            ns_per_op: median(samples),
        });
        if !args.json {
            print_row(rows.last().expect("row just pushed"));
        }
    }
    if args.json {
        print_json("explain", &args, &rows);
    }
}
