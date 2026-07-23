use std::time::Instant;
use qql_core::parser::Parser;
use qql_plan::routing;
const Q: &str = "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}";
fn main() {
    let n = 500_000;
    let start = Instant::now();
    for _ in 0..n { let _ = Parser::parse(Q).unwrap(); }
    let parse = start.elapsed();
    let start = Instant::now();
    for _ in 0..n { let stmt = Parser::parse(Q).unwrap(); let _ = routing::route(&stmt); }
    let route = start.elapsed();
    let start = Instant::now();
    for _ in 0..n { let stmt = Parser::parse(Q).unwrap(); let r = routing::route(&stmt); let _ = r.body_json(); }
    let json = start.elapsed();
    let p_ns = parse.as_nanos() as f64 / n as f64;
    let r_ns = route.as_nanos() as f64 / n as f64;
    let j_ns = json.as_nanos() as f64 / n as f64;
    println!("UPSERT Pipeline ({} iterations):", n);
    println!("  Parse only:    {:8.0} ns   ({:12.0} ops/s)", p_ns, 1e9/p_ns);
    println!("  + route:       {:8.0} ns   ({:12.0} ops/s)", r_ns, 1e9/r_ns);
    println!("  + body_json:   {:8.0} ns   ({:12.0} ops/s)", j_ns, 1e9/j_ns);
    println!("  Route cost: {:5.0} ns, JSON cost: {:5.0} ns", r_ns - p_ns, j_ns - r_ns);
}
