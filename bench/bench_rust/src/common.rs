//! Shared harness for the `bench_rust` binaries.
//!
//! Zero extra dependencies: CLI parsing (`--iterations/--reps/--filter/--json`),
//! median-of-reps timing, `bench/queries.json` loading, and table/JSON output.
//!
//! Lives under `src/` (not `src/bin/`) so Cargo does not treat it as a binary
//! target. Bins include it with `#[path = "../common.rs"] mod common;`.
//!
//! Each binary monomorphizes its own copy of this module and uses a different
//! subset, so unused helpers are expected — not dead code to delete.
//! (A `src/lib.rs` would avoid the duplication but adds a lib target to a
//! bins-only bench crate; the duplication is a few KB of bench-only code.)
#![allow(dead_code)]

use std::hint::black_box;
use std::time::Instant;

/// Parsed CLI options shared by every bench binary.
pub struct Args {
    /// Timed iterations per rep.
    pub iterations: usize,
    /// Measurement reps; the reported value is the median rep.
    pub reps: usize,
    /// Case-insensitive substring filter on query names.
    pub filter: Option<String>,
    /// Emit one JSON object instead of the human table.
    pub json: bool,
}

/// Parse `std::env::args`. Exits 0 on `--help`, 2 on misuse.
pub fn parse_args(bin: &str, default_iterations: usize, default_reps: usize) -> Args {
    let mut args = Args {
        iterations: default_iterations,
        reps: default_reps,
        filter: None,
        json: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "usage: {bin} [--iterations N] [--reps N] [--filter SUBSTR] [--json]\n\n\
                     Benchmarks every query in bench/queries.json (median of N reps).\n\
                     defaults: --iterations {default_iterations} --reps {default_reps}"
                );
                std::process::exit(0);
            }
            "--json" => args.json = true,
            "--iterations" | "--reps" | "--filter" => {
                let value = it.next().unwrap_or_else(|| {
                    eprintln!("{bin}: {arg} requires a value");
                    std::process::exit(2);
                });
                match arg.as_str() {
                    "--iterations" => {
                        args.iterations = value.parse().unwrap_or_else(|_| {
                            eprintln!("{bin}: invalid --iterations '{value}'");
                            std::process::exit(2);
                        });
                    }
                    "--reps" => {
                        args.reps = value.parse().unwrap_or_else(|_| {
                            eprintln!("{bin}: invalid --reps '{value}'");
                            std::process::exit(2);
                        });
                    }
                    _ => args.filter = Some(value),
                }
            }
            other => {
                eprintln!("{bin}: unknown argument '{other}' (try --help)");
                std::process::exit(2);
            }
        }
    }
    if args.iterations == 0 || args.reps == 0 {
        eprintln!("{bin}: --iterations and --reps must be >= 1");
        std::process::exit(2);
    }
    args
}

/// One benchmark query from `bench/queries.json`.
pub struct BenchQuery {
    /// Stable label (e.g. `Simple`).
    pub name: String,
    /// QQL source. May contain `:name` placeholders when `params` is set.
    pub qql: String,
    /// Optional JSON params for the 0.3.2 bind path (`Bound` query).
    pub params: Option<serde_json::Value>,
    /// Optional executable counterpart for the mock-executor suite (`e2e.rs`
    /// uses this when present, otherwise `qql`).
    pub e2e: Option<String>,
}

/// Load the shared corpus. Embedded at compile time so binaries stay portable.
pub fn load_queries() -> Vec<BenchQuery> {
    const RAW: &str = include_str!("../../queries.json");
    let root: serde_json::Value = serde_json::from_str(RAW).expect("queries.json must parse");
    root["queries"]
        .as_array()
        .expect("queries.json needs a queries array")
        .iter()
        .map(|entry| BenchQuery {
            name: entry["name"]
                .as_str()
                .expect("query entry needs a name")
                .to_string(),
            qql: entry["qql"]
                .as_str()
                .expect("query entry needs qql")
                .to_string(),
            params: entry.get("params").cloned(),
            e2e: entry
                .get("e2e")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        })
        .collect()
}

/// Keep queries whose name contains `filter` (case-insensitive). Exits 2 when
/// the filter matches nothing, so typos fail loudly instead of benching zero rows.
pub fn apply_filter(queries: Vec<BenchQuery>, filter: Option<&str>) -> Vec<BenchQuery> {
    let Some(f) = filter else {
        return queries;
    };
    let needle = f.to_ascii_lowercase();
    let kept: Vec<BenchQuery> = queries
        .into_iter()
        .filter(|q| q.name.to_ascii_lowercase().contains(&needle))
        .collect();
    if kept.is_empty() {
        eprintln!("no queries match filter '{f}' (see bench/queries.json)");
        std::process::exit(2);
    }
    kept
}

/// Median of a sample. Sorts in place; empty input yields 0.0.
pub fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = xs.len() / 2;
    if xs.len() % 2 == 1 {
        xs[mid]
    } else {
        (xs[mid - 1] + xs[mid]) / 2.0
    }
}

/// Time `work` for `iterations` timed iterations after `warmup` untimed ones.
/// The closure's return value is sunk through `black_box` so the optimizer
/// cannot discard the measured work.
pub fn time_op<T>(warmup: usize, iterations: usize, mut work: impl FnMut() -> T) -> f64 {
    for _ in 0..warmup {
        black_box(work());
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(work());
    }
    start.elapsed().as_nanos() as f64 / iterations as f64
}

/// Run `reps` timed reps of `work`, returning the per-rep ns/op samples.
/// The caller reports [`median`] of the samples.
pub fn run_reps<T>(
    warmup: usize,
    iterations: usize,
    reps: usize,
    mut work: impl FnMut() -> T,
) -> Vec<f64> {
    (0..reps)
        .map(|_| time_op(warmup, iterations, &mut work))
        .collect()
}

/// One result row for table/JSON output.
pub struct Row {
    /// Query or operation label.
    pub name: String,
    /// Median ns/op across reps.
    pub ns_per_op: f64,
}

/// Render ops/s from ns/op.
pub fn ops_per_sec(ns_per_op: f64) -> f64 {
    if ns_per_op > 0.0 {
        1_000_000_000.0 / ns_per_op
    } else {
        0.0
    }
}

/// Human table header, matching the historical `Query ns/op ops/s` layout.
pub fn print_header(bin: &str, args: &Args) {
    println!(
        "{bin}  |  {} iterations x {} reps (median)\n",
        args.iterations, args.reps
    );
    println!("{:<20} {:>10} {:>12}", "Query", "ns/op", "ops/s");
    println!("{}", "-".repeat(46));
}

/// Human table row.
pub fn print_row(row: &Row) {
    println!(
        "{:<20} {:>10.0} {:>12.0}",
        row.name,
        row.ns_per_op,
        ops_per_sec(row.ns_per_op)
    );
}

/// Machine-readable report: one JSON object on stdout.
pub fn print_json(bin: &str, args: &Args, rows: &[Row]) {
    let results: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "query": row.name,
                "ns_per_op": row.ns_per_op,
                "ops_per_sec": ops_per_sec(row.ns_per_op),
            })
        })
        .collect();
    let report = serde_json::json!({
        "bin": bin,
        "bin_version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "iterations": args.iterations,
        "reps": args.reps,
        "results": results,
    });
    println!("{report}");
}
