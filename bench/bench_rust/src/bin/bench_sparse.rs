//! BM25 sparse embedding throughput (`qql::sparse`).
//!
//! usage: `bench_sparse [--iterations N] [--reps N] [--filter SUBSTR] [--json]`

#[path = "../common.rs"]
mod common;

use common::{Row, median, parse_args, print_header, print_json, print_row, run_reps};

/// A named timed closure. Borrows its input texts from the caller.
type Case<'a> = (&'static str, Box<dyn FnMut() + 'a>);

fn main() {
    let args = parse_args("bench_sparse", 100_000, 5);
    let filter = args.filter.as_deref().map(str::to_ascii_lowercase);
    let doc_text = "The quick brown fox jumps over the lazy dog near the riverbank with acute fever and cough symptoms.";
    let query_text = "acute fever cough treatment";

    let mut cases: Vec<Case<'_>> = vec![
        (
            "Build Document",
            Box::new(|| {
                qql::sparse::embed_document(doc_text);
            }),
        ),
        (
            "Build Query",
            Box::new(|| {
                qql::sparse::embed_query(query_text);
            }),
        ),
    ];
    if let Some(needle) = &filter {
        cases.retain(|(name, _)| name.to_ascii_lowercase().contains(needle));
        if cases.is_empty() {
            eprintln!("no sparse cases match filter '{needle}'");
            std::process::exit(2);
        }
    }

    if !args.json {
        print_header("Rust BM25 sparse embeddings", &args);
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
        print_json("bench_sparse", &args, &rows);
    }
}
