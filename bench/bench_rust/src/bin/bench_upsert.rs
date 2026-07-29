use qql_core::parser::Parser;
use qql_plan::routing;
use std::{hint::black_box, time::Instant};

const Q: &str = "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}";

fn main() {
    let n = 500_000;

    for _ in 0..1_000 {
        black_box(Parser::parse(Q).unwrap());
    }

    let start = Instant::now();
    for _ in 0..n {
        black_box(Parser::parse(Q).unwrap());
    }
    let parse = start.elapsed();
    let start = Instant::now();
    for _ in 0..n {
        let stmt = Parser::parse(Q).unwrap();
        black_box(routing::try_route(&stmt).unwrap());
    }
    let route = start.elapsed();
    let start = Instant::now();
    for _ in 0..n {
        let stmt = Parser::parse(Q).unwrap();
        let route = routing::try_route(&stmt).unwrap();
        black_box(route.body_json());
    }
    let json = start.elapsed();
    let p_ns = parse.as_nanos() as f64 / n as f64;
    let r_ns = route.as_nanos() as f64 / n as f64;
    let j_ns = json.as_nanos() as f64 / n as f64;
    println!("UPSERT Pipeline ({} iterations):", n);
    println!(
        "  Parse only:    {:8.0} ns   ({:12.0} ops/s)",
        p_ns,
        1e9 / p_ns
    );
    println!(
        "  + route:       {:8.0} ns   ({:12.0} ops/s)",
        r_ns,
        1e9 / r_ns
    );
    println!(
        "  + body_json:   {:8.0} ns   ({:12.0} ops/s)",
        j_ns,
        1e9 / j_ns
    );
    println!(
        "  Route cost: {:5.0} ns, JSON cost: {:5.0} ns",
        r_ns - p_ns,
        j_ns - r_ns
    );
}
