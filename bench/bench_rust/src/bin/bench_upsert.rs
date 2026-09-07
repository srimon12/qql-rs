//! UPSERT pipeline breakdown: parse → route projection → JSON body.
//!
//! The `Upsert` query comes from `bench/queries.json` so it cannot drift from
//! the other suites.
//!
//! usage: `bench_upsert [--iterations N] [--reps N] [--filter SUBSTR] [--json]`

#[path = "../common.rs"]
mod common;

use common::{Row, median, parse_args, print_header, print_json, print_row, run_reps};
use qql_core::parser::Parser;
use qql_plan::routing;

fn main() {
    let args = parse_args("bench_upsert", 200_000, 5);
    let upsert_qql = common::load_queries()
        .into_iter()
        .find(|q| q.name == "Upsert")
        .expect("bench/queries.json must contain an 'Upsert' entry")
        .qql;
    // Leak to a 'static str so the timed closures avoid per-iter cloning.
    let query: &'static str = Box::leak(upsert_qql.into_boxed_str());

    if !args.json {
        print_header("Rust UPSERT pipeline", &args);
    }
    let mut rows = Vec::with_capacity(3);

    let samples = run_reps(1_000, args.iterations, args.reps, || {
        Parser::parse(query).expect("Upsert must parse")
    });
    rows.push(Row {
        name: "Parse only".to_string(),
        ns_per_op: median(samples),
    });

    let samples = run_reps(1_000, args.iterations, args.reps, || {
        let stmt = Parser::parse(query).expect("Upsert must parse");
        routing::try_route(&stmt).expect("Upsert must route")
    });
    rows.push(Row {
        name: "+ route".to_string(),
        ns_per_op: median(samples),
    });

    let samples = run_reps(1_000, args.iterations, args.reps, || {
        let stmt = Parser::parse(query).expect("Upsert must parse");
        let route = routing::try_route(&stmt).expect("Upsert must route");
        route.body_json()
    });
    rows.push(Row {
        name: "+ body_json".to_string(),
        ns_per_op: median(samples),
    });

    if args.json {
        print_json("bench_upsert", &args, &rows);
    } else {
        for row in &rows {
            print_row(row);
        }
        println!(
            "  Route cost: {:5.0} ns, JSON cost: {:5.0} ns",
            rows[1].ns_per_op - rows[0].ns_per_op,
            rows[2].ns_per_op - rows[1].ns_per_op
        );
    }
}
