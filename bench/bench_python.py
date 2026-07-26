"""Benchmark pyqql parse and E2E explain across query types."""
import time
import sys

sys.path.insert(0, "../target/release")
import pyqql

QUERIES = [
    ("Simple", "QUERY 'search' FROM docs LIMIT 10"),
    ("Hybrid", "QUERY HYBRID TEXT 'search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10"),
    ("Full", "QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2"),
    ("CTE_Prefetch", ("WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) "
                      "QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10")),
    ("CreateCollection", "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"),
    ("Upsert", "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}"),
    ("DeleteWhere", "DELETE FROM docs WHERE category = 'archived'"),
    ("OrderBy", "QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20"),
    ("WithPayload", "QUERY 'search' FROM docs WITH PAYLOAD INCLUDE (title, body) WITH VECTOR (dense) LIMIT 10"),
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
