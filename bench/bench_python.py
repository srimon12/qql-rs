"""pyqql throughput across the shared bench/queries.json corpus.

Median-of-reps timing (default 50k iterations x 3 reps). Every query benches
parse / parse_json / explain; the Bound query additionally benches the 0.3.2
prepared-statement path (bind, compile_query, is_valid).

Usage:
    python3 bench/bench_python.py [--iterations N] [--reps N]
                                   [--filter SUBSTR] [--json]
"""

import argparse
import gc
import json
import sys
import time
from pathlib import Path

try:
    import pyqql
except ImportError:
    sys.exit("pyqql is not installed (cd crates/pyqql && maturin develop --release)")

BENCH_DIR = Path(__file__).resolve().parent
CORPUS = json.loads((BENCH_DIR / "queries.json").read_text())["queries"]


def median(xs):
    xs = sorted(xs)
    mid = len(xs) // 2
    return xs[mid] if len(xs) % 2 else (xs[mid - 1] + xs[mid]) / 2.0


def time_op(fn, warmup, iterations):
    for _ in range(warmup):
        fn()
    gc.disable()
    try:
        start = time.perf_counter()
        for _ in range(iterations):
            fn()
        elapsed = time.perf_counter() - start
    finally:
        gc.enable()
    return iterations / elapsed  # ops/s


def bench_ops(fn, warmup, iterations, reps):
    """Median ops/s across reps (each rep reports ops/s; median of rates)."""
    return median([time_op(fn, warmup, iterations) for _ in range(reps)])


def main():
    parser = argparse.ArgumentParser(description="pyqql bench (median of reps)")
    parser.add_argument("--iterations", type=int, default=50_000)
    parser.add_argument("--reps", type=int, default=3)
    parser.add_argument("--filter", default=None, help="case-insensitive substring on query names")
    parser.add_argument("--json", action="store_true", help="emit one JSON object")
    args = parser.parse_args()

    queries = CORPUS
    if args.filter:
        needle = args.filter.lower()
        queries = [q for q in queries if needle in q["name"].lower()]
        if not queries:
            sys.exit(f"no queries match filter '{args.filter}' (see bench/queries.json)")

    rows = []
    for q in queries:
        name, src = q["name"], q["qql"]
        rows.append(
            {
                "query": name,
                "parse": bench_ops(lambda: pyqql.parse(src), 1000, args.iterations, args.reps),
                "parse_json": bench_ops(
                    lambda: pyqql.parse_json(src), 1000, args.iterations, args.reps
                ),
                "explain": bench_ops(
                    lambda: pyqql.explain(src), 1000, args.iterations, args.reps
                ),
            }
        )

    # 0.3.2 prepared-statement path on the Bound query (if not filtered out).
    bound = next((q for q in queries if q["name"] == "Bound"), None)
    bind_row = None
    if bound is not None:
        src, params = bound["qql"], bound["params"]
        bind_row = {
            "bind": bench_ops(lambda: pyqql.bind(src, params), 1000, args.iterations, args.reps),
            "compile_query": bench_ops(
                lambda: pyqql.compile_query(src, params), 1000, args.iterations, args.reps
            ),
            "is_valid": bench_ops(
                lambda: pyqql.is_valid(src), 1000, args.iterations, args.reps
            ),
        }

    if args.json:
        print(
            json.dumps(
                {
                    "bin": "bench_python",
                    "impl": f"pyqql {getattr(pyqql, '__version__', '?')}",
                    "iterations": args.iterations,
                    "reps": args.reps,
                    "results": rows,
                    "bound": bind_row,
                }
            )
        )
        return

    print(f"Python pyqql {getattr(pyqql, '__version__', '')}  |  {args.iterations} iterations x {args.reps} reps (median)\n")
    print(f"{'Query':<20} | {'Parse':>15} | {'ParseJson':>15} | {'Explain':>15}")
    print("-" * 74)
    for r in rows:
        print(
            f"{r['query']:<20} | {r['parse']:>15.0f} | {r['parse_json']:>15.0f} | {r['explain']:>15.0f}"
        )
    if bind_row is not None:
        print("\nBound query prepared-statement path (ops/s, median):")
        for key, value in bind_row.items():
            print(f"  {key:<15} {value:>15.0f}")


if __name__ == "__main__":
    main()
