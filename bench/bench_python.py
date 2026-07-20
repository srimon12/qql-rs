"""Benchmark pyqql parse and E2E explain across query types."""
import time
import sys

sys.path.insert(0, "../target/release")
import pyqql

QUERIES = [
    ("Simple", "QUERY 'search' FROM docs LIMIT 10"),
    ("Hybrid", "QUERY 'search' FROM docs LIMIT 10 USING HYBRID"),
    ("Full", "QUERY 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128, exact = true)"),
    ("CTE_Prefetch", ("WITH a AS (QUERY 'search' USING dense LIMIT 100 WHERE category = 'tech'), b AS (QUERY 'search' USING sparse LIMIT 100)\n"
                      "QUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.8, b SCORE THRESHOLD 0.5) FUSION RRF")),
    ("CreateCollection", "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"),
    ("Insert", "INSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}"),
    ("DeleteWhere", "DELETE FROM docs WHERE category = 'archived'"),
    ("OrderBy", "QUERY ORDER BY created_at DESC FROM docs LIMIT 20 WHERE status = 'active'"),
    ("WithPayload", "QUERY 'search' FROM docs LIMIT 10 WITH PAYLOAD (include = ['title', 'body']) WITH VECTORS ('dense')"),
]

def bench_parse(q, iterations):
    for _ in range(100):
        pyqql.parse(q)
    start = time.perf_counter()
    for _ in range(iterations):
        pyqql.parse(q)
    elapsed = time.perf_counter() - start
    return iterations / elapsed

def bench_e2e(q, iterations):
    # Explain constructs the full execution payload offline (E2E pipeline)
    for _ in range(100):
        pyqql.explain(q)
    start = time.perf_counter()
    for _ in range(iterations):
        pyqql.explain(q)
    elapsed = time.perf_counter() - start
    return iterations / elapsed

if __name__ == "__main__":
    iterations = 10_000
    print(f"Python pyqql  |  {iterations} iterations each\n")
    print(f"{'Query':<20} | {'Parse (ops/s)':>15} | {'E2E (ops/s)':>15}")
    print("-" * 58)

    for name, q in QUERIES:
        parse_ops = bench_parse(q, iterations)
        e2e_ops = bench_e2e(q, iterations)
        print(f"{name:<20} | {parse_ops:>15.0f} | {e2e_ops:>15.0f}")
