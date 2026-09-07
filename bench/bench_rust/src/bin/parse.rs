//! Parser throughput: lex + parse each query in `bench/queries.json`.
//!
//! usage: `parse [--iterations N] [--reps N] [--filter SUBSTR] [--json]`

#[path = "../common.rs"]
mod common;

use common::{
    Row, apply_filter, load_queries, median, parse_args, print_header, print_json, print_row,
    run_reps,
};

fn main() {
    let args = parse_args("parse", 100_000, 5);
    let queries = apply_filter(load_queries(), args.filter.as_deref());

    if !args.json {
        print_header("Rust qql-rs parse only", &args);
    }
    let mut rows = Vec::with_capacity(queries.len());
    for query in &queries {
        let samples = run_reps(1_000, args.iterations, args.reps, || {
            qql_core::parser::Parser::parse(&query.qql).expect("benchmark query must parse")
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
        print_json("parse", &args, &rows);
    }
}
