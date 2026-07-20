use std::time::Instant;

fn main() {
    let doc_text = "The quick brown fox jumps over the lazy dog near the riverbank with acute fever and cough symptoms.";
    let query_text = "acute fever cough treatment";

    let iterations = 100_000;

    // Warmup
    for _ in 0..1_000 {
        let _ = qql::sparse::build_document_default(doc_text);
        let _ = qql::sparse::build_query(query_text);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = qql::sparse::build_document_default(doc_text);
    }
    let elapsed_doc = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = qql::sparse::build_query(query_text);
    }
    let elapsed_query = start.elapsed();

    println!("=== RUST BM25 BENCHMARK (100,000 iterations) ===");
    println!(
        "Build Document: {:.2?} ({:.0} ops/sec)",
        elapsed_doc,
        iterations as f64 / elapsed_doc.as_secs_f64()
    );
    println!(
        "Build Query:    {:.2?} ({:.0} ops/sec)",
        elapsed_query,
        iterations as f64 / elapsed_query.as_secs_f64()
    );
}
