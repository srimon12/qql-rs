#!/usr/bin/env python3
"""Head-to-head: pyqql-edge vs qdrant-edge-py (both in-process, qdrant-edge 0.8).

Methodology mirrors ../vs-qdrant: deterministic corpus re-used byte-identically,
result parity asserted before any timing is reported, interleaved repetitions,
median/p95 of per-call samples. No network, no Qdrant server — both contenders
are the same qdrant-edge core behind a different application layer.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import importlib.metadata
import os
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import numpy as np  # noqa: E402

import corpus  # noqa: E402
from config import (  # noqa: E402
    COLBERT_ITERS,
    COLBERT_REPS,
    LIMIT,
    PREPARED_ITERS,
    PYQQ_EDGE_PKG,
    READS_ITERS,
    READS_REPS,
    RESULTS,
    SCROLL_BATCH,
    SCROLL_PAGES,
    WORK,
)
from official import OfficialEdge  # noqa: E402
from qql import QqlEdge  # noqa: E402

B, L = "berlin", "legal"


# ----------------------------------------------------------------- timing ----
def _timed(fn, iters: int) -> list[float]:
    samples = []
    for _ in range(iters):
        t0 = time.perf_counter()
        fn()
        samples.append(time.perf_counter() - t0)
    return samples


def measure_pair(off_fn, qql_fn, reps: int, iters: int) -> tuple[list, list]:
    off_fn()  # warmup
    qql_fn()
    off_samples, qql_samples = [], []
    for rep in range(reps):
        if rep % 2 == 0:
            off_samples += _timed(off_fn, iters)
            qql_samples += _timed(qql_fn, iters)
        else:
            qql_samples += _timed(qql_fn, iters)
            off_samples += _timed(off_fn, iters)
    return off_samples, qql_samples


def summarize(samples: list[float]) -> dict:
    ordered = sorted(samples)
    n = len(ordered)
    median = ordered[n // 2]
    p95 = ordered[min(n - 1, int(0.95 * n))]
    return {
        "ops_s": 1.0 / median,
        "p50_ms": median * 1000.0,
        "p95_ms": p95 * 1000.0,
        "n": n,
    }


# ----------------------------------------------------------------- parity ----
def cmp_hits(off, qql) -> dict:
    o = [(h.id, h.score) for h in off]
    q = [(h.id, h.score) for h in qql]
    o_ids, q_ids = [x for x, _ in o], [x for x, _ in q]
    ids_equal = o_ids == q_ids
    overlap = len(set(o_ids) & set(q_ids)) / max(1, len(set(o_ids) | set(q_ids)))
    if ids_equal:
        score_max = max((abs(a - b) for (_, a), (_, b) in zip(o, q)), default=0.0)
        ok = score_max <= 2e-4
    else:
        score_max = None
        ok = overlap >= 0.8
    return {
        "kind": "hits",
        "ids_equal": ids_equal,
        "overlap": round(overlap, 3),
        "score_max_diff": None if score_max is None else round(score_max, 6),
        "ok": ok,
    }


def cmp_exact(off, qql) -> dict:
    return {
        "kind": "exact",
        "ok": off == qql,
        "official_len": len(off) if hasattr(off, "__len__") else None,
    }


def cmp_records(off, qql) -> dict:
    o = [(record.id, record.payload) for record in off]
    q = [(hit.id, hit.payload) for hit in qql]
    ids_equal = [i for i, _ in o] == [i for i, _ in q]
    payloads_equal = all(a == b for (_, a), (_, b) in zip(o, q))
    return {
        "kind": "records",
        "ids_equal": ids_equal,
        "payloads_equal": payloads_equal,
        "ok": ids_equal and payloads_equal,
    }


# ------------------------------------------------------------------ report ----
def fmt_row(name: str, off: dict, qql: dict, parity: dict) -> str:
    return (
        f"| {name} | {off['ops_s']:.1f} | {qql['ops_s']:.1f} | "
        f"{qql['ops_s'] / off['ops_s']:.2f}x | {off['p50_ms']:.3f} ms | "
        f"{qql['p50_ms']:.3f} ms | {'ok' if parity['ok'] else 'FAIL'} |"
    )


def loc(path: Path) -> int:
    """Non-empty, non-comment lines — a rough application-code size proxy."""
    count = 0
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            count += 1
    return count


def vec_digest(value) -> str:
    """Stable SHA256 of the exact query input handed to both contenders.

    Engine scenarios pass the *same precomputed artifact* (from
    ../vs-qdrant/data/queries.json) to both sides; the digest recorded here
    makes that checkable in results/edge.json.
    """
    blob = json.dumps(value, separators=(",", ":"), sort_keys=True).encode()
    return hashlib.sha256(blob).hexdigest()[:16]


# --------------------------------------------------------------------- run ----
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reps", type=int, default=READS_REPS)
    parser.add_argument("--iters", type=int, default=READS_ITERS)
    parser.add_argument(
        "--no-ingest",
        action="store_true",
        help="reuse existing shard dirs under work/ (skip rebuild + ingest)",
    )
    parser.add_argument("--skip-colbert", action="store_true")
    parser.add_argument("--skip-threads", action="store_true")
    parser.add_argument("--skip-optimize", action="store_true")
    args = parser.parse_args()

    RESULTS.mkdir(exist_ok=True)
    if not args.no_ingest:
        shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True, exist_ok=True)

    data = corpus.load()
    results: dict = {
        "env": {
            "python": sys.version.split()[0],
            "qdrant_edge_py": importlib.metadata.version("qdrant-edge-py"),
            "pyqql_edge": __import__("pyqql_edge").__version__,
            "points": {"berlin": len(data["docs"]["berlin"]), "legal": len(data["docs"]["legal"])},
        },
        "ingest": {},
        "reads": {},
        "threading": {},
        "optimize": {},
        "startup": {},
        "cold_import_ms": {},
    }

    t0 = time.perf_counter()
    official = OfficialEdge(WORK / "official")
    official_startup = time.perf_counter() - t0
    t0 = time.perf_counter()
    qql = QqlEdge(WORK / "qql")
    qql_startup = time.perf_counter() - t0
    results["startup"] = {
        "official_edge_shards_s": official_startup,
        "qql_executor_and_embedder_s": qql_startup,
    }
    print(
        f"startup: official={official_startup * 1000:.0f} ms  "
        f"qql={qql_startup * 1000:.0f} ms (includes FastEmbed executor init)"
    )

    try:
        # ------------------------------------------------------------ setup --
        if not args.no_ingest:
            t0 = time.perf_counter()
            official.create_berlin(B)
            qql.create_berlin(B)
            official.create_legal(L)
            qql.create_legal(L)
            print(f"collections created in {time.perf_counter() - t0:.2f} s")

            # --------------------------------------------------------- ingest --
            off_prep, off_submit = official.ingest_berlin(
                B, data["docs"][B], data["dense"][B], data["sparse"][B]
            )
            ql_prep, ql_submit = qql.ingest_berlin(
                B, data["docs"][B], data["dense"][B], data["sparse"][B]
            )
            results["ingest"]["berlin"] = {
                "points": len(data["docs"][B]),
                "official_prep_s": off_prep,
                "official_submit_s": off_submit,
                "qql_prep_s": ql_prep,
                "qql_submit_s": ql_submit,
            }
            print(
                f"ingest berlin: official {off_submit:.2f} s submit "
                f"(+{off_prep:.2f} s prep) | qql {ql_submit:.2f} s submit "
                f"(+{ql_prep:.2f} s prep)"
            )

            off_prep, off_submit = official.ingest_legal(
                L,
                data["docs"][L],
                data["dense"][L],
                data["sparse"][L],
                data["colbert"]["flat"],
                data["colbert"]["lens"],
            )
            ql_prep, ql_submit = qql.ingest_legal(
                L,
                data["docs"][L],
                data["dense"][L],
                data["sparse"][L],
                data["colbert"]["flat"],
                data["colbert"]["lens"],
            )
            results["ingest"]["legal"] = {
                "points": len(data["docs"][L]),
                "official_prep_s": off_prep,
                "official_submit_s": off_submit,
                "qql_prep_s": ql_prep,
                "qql_submit_s": ql_submit,
            }
            print(
                f"ingest legal:  official {off_submit:.2f} s submit "
                f"(+{off_prep:.2f} s prep) | qql {ql_submit:.2f} s submit "
                f"(+{ql_prep:.2f} s prep)"
            )
        else:
            official.load(B)
            official.load(L)

        # -------------------------------------------------------- raw probe --
        off_point = official.retrieve_points(B, [1])[0]
        ql_point = qql.retrieve_points(B, [1])[0]
        results["probe"] = {
            "id": off_point.id,
            "payload_equal": off_point.payload == ql_point.payload,
        }
        print(f"probe point 1: payload equal = {results['probe']['payload_equal']}")

        # ------------------------------------------------------------ reads --
        # Groups:
        #   engine — both contenders receive the *same precomputed query
        #            vector* (or ids/filter) from ../vs-qdrant/data; no
        #            embedding happens in either timed call: strict
        #            head-to-head over the same qdrant-edge core.
        #   text   — input is raw text; each side runs its own embedder
        #            (QQL in-path, official via fastembed/Bm25), then the same
        #            engine. End-to-end layer comparison, NOT engine-only.
        qb = data["queries"][B][0]
        qlg = data["queries"][L][0]
        ids = [doc["id"] for doc in data["docs"][B][:LIMIT]]
        batch_vecs = [q["dense"] for q in data["queries"][B]]
        prepared_vecs = [q["dense"] for q in data["queries"][B]]
        # Best idiom per side for the *same* f32 values: edge-py is measured
        # faster with Python lists, pyqql with numpy rows (F32Array fast path).
        dense_np = np.asarray(qb["dense"], dtype=np.float32)
        batch_np = np.asarray(batch_vecs, dtype=np.float32)
        prepared_np = np.asarray(prepared_vecs, dtype=np.float32)
        colbert_flat = np.asarray(qlg["colbert"], dtype=np.float32).reshape(-1)

        scenarios = [
            {
                "name": "query_dense",
                "group": "engine",
                "query_input": "dense vector (precomputed)",
                "digest": vec_digest(qb["dense"]),
                "off": lambda: official.query_dense(B, qb["dense"]),
                "qql": lambda: qql.query_dense(B, dense_np),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "query_dense_filtered",
                "group": "engine",
                "query_input": "dense vector (precomputed) + price/guests filter",
                "digest": vec_digest(
                    {"dense": qb["dense"], "price_lt": 150.0, "guests_gte": 2}
                ),
                "off": lambda: official.query_dense_filtered(B, qb["dense"]),
                "qql": lambda: qql.query_dense_filtered(B, dense_np),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "query_sparse",
                "group": "engine",
                "query_input": "BM25 sparse vector (precomputed)",
                "digest": vec_digest(qb["sparse"]),
                "off": lambda: official.query_sparse(B, qb["sparse"]),
                "qql": lambda: qql.query_sparse(B, qb["sparse"]),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "query_hybrid",
                "group": "engine",
                "query_input": "RRF: precomputed dense + precomputed BM25 sparse",
                "digest": vec_digest(
                    {"dense": qb["dense"], "sparse": qb["sparse"], "prefetch_limit": 50}
                ),
                "off": lambda: official.query_hybrid(B, qb["dense"], qb["sparse"]),
                "qql": lambda: qql.query_hybrid(B, dense_np, qb["sparse"]),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "count_berlin",
                "group": "engine",
                "query_input": "price < 150.0 (no vector)",
                "digest": vec_digest({"price_lt": 150.0}),
                "off": lambda: official.count_berlin(B),
                "qql": lambda: qql.count_berlin(B),
                "cmp": cmp_exact,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "count_legal",
                "group": "engine",
                "query_input": "year >= 2010 (no vector)",
                "digest": vec_digest({"year_gte": 2010}),
                "off": lambda: official.count_legal(L),
                "qql": lambda: qql.count_legal(L),
                "cmp": cmp_exact,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "facet_district",
                "group": "engine",
                "query_input": "district facet, limit 20, exact",
                "digest": vec_digest({"facet": "district", "limit": 20}),
                "off": lambda: official.facet_district(B),
                "qql": lambda: qql.facet_district(B),
                "cmp": cmp_exact,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "scroll_pages",
                "group": "engine",
                "query_input": "3 pages x 256 ids (no vector)",
                "digest": vec_digest({"pages": SCROLL_PAGES, "batch": SCROLL_BATCH}),
                "off": lambda: official.scroll_pages(B, SCROLL_PAGES, SCROLL_BATCH),
                "qql": lambda: qql.scroll_pages(B, SCROLL_PAGES, SCROLL_BATCH),
                "cmp": cmp_exact,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "retrieve_points",
                "group": "engine",
                "query_input": "10 point ids",
                "digest": vec_digest(ids),
                "off": lambda: official.retrieve_points(B, ids),
                "qql": lambda: qql.retrieve_points(B, ids),
                "cmp": cmp_records,
                "reps": args.reps,
                "iters": args.iters,
            },
            {
                "name": "prepared_rerun",
                "group": "engine",
                "query_input": "4 precomputed dense vectors, prepared stmt",
                "digest": vec_digest(prepared_vecs),
                "off": lambda: official.prepared_rerun(B, prepared_vecs),
                "qql": lambda: qql.prepared_rerun(B, prepared_np),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": max(1, PREPARED_ITERS // 4),
            },
            {
                "name": "batch_reads",
                "group": "engine",
                "query_input": "4 precomputed dense vectors, one batch call",
                "digest": vec_digest(batch_vecs),
                "off": lambda: official.batch_reads(B, batch_vecs),
                "qql": lambda: qql.batch_reads(B, batch_np),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": args.iters,
            },
        ]
        if not args.skip_colbert:
            scenarios.append(
                {
                    "name": "query_colbert",
                    "group": "engine",
                    "query_input": "ColBERT multivector (precomputed)",
                    "digest": vec_digest(qlg["colbert"]),
                    "off": lambda: official.query_colbert(L, qlg["colbert"]),
                    "qql": lambda: qql.query_colbert(
                        L, {"data": colbert_flat, "dim": 128}
                    ),
                    "cmp": cmp_hits,
                    "reps": COLBERT_REPS,
                    "iters": COLBERT_ITERS,
                }
            )

        # In-process TEXT -> vector scenarios: the QQL side embeds inside
        # execute(); the official side must bring its own embedder.
        t0 = time.perf_counter()
        official.load_embedder()
        results["startup"]["official_embedder_s"] = time.perf_counter() - t0
        scenarios.append(
            {
                "name": "text_dense",
                "group": "text",
                "query_input": "raw text; fastembed-rs vs fastembed Python",
                "digest": vec_digest(qb["text"]),
                "off": lambda: official.text_dense(B, qb["text"]),
                "qql": lambda: qql.text_dense(B, qb["text"]),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": max(5, args.iters // 5),
            }
        )
        scenarios.append(
            {
                "name": "text_sparse",
                "group": "text",
                "query_input": "raw text; QQL BM25 vs edge Bm25",
                "digest": vec_digest(qb["text"]),
                "off": lambda: official.text_sparse(B, qb["text"]),
                "qql": lambda: qql.text_sparse(B, qb["text"]),
                "cmp": cmp_hits,
                "reps": args.reps,
                "iters": args.iters,
            }
        )

        last_group = None
        for scenario in scenarios:
            name = scenario["name"]
            if scenario["group"] != last_group:
                print(f"\n[{scenario['group']}]")
                last_group = scenario["group"]
            try:
                parity = scenario["cmp"](scenario["off"](), scenario["qql"]())
                off_samples, qql_samples = measure_pair(
                    scenario["off"], scenario["qql"], scenario["reps"], scenario["iters"]
                )
            except Exception as error:  # noqa: BLE001 - report, don't abort the suite
                results["reads"][name] = {"error": f"{type(error).__name__}: {error}"}
                print(f"{name}: ERROR {error}")
                continue
            off, ql = summarize(off_samples), summarize(qql_samples)
            results["reads"][name] = {
                "group": scenario["group"],
                "query_input": scenario["query_input"],
                "query_input_sha256_16": scenario["digest"],
                "official": off,
                "qql": ql,
                "parity": parity,
            }
            print(fmt_row(name, off, ql, parity))

        # ------------------------------------------------------- threading --
        # The official binding never releases the GIL (no allow_threads in
        # qdrant-edge-py), so concurrent Python threads serialize. pyqql-edge
        # calls py.detach(), letting engine searches run in parallel.
        if not args.skip_threads:
            print("\n[threading]  aggregate throughput, same shard, 40 queries/thread")
            results["threading"] = {}

            def off_call():
                official.query_dense(B, qb["dense"])

            def qql_call():
                qql.query_dense(B, dense_np)

            for workers in (1, 2, 4, 8):
                row = {}
                for label, fn in (("official", off_call), ("qql", qql_call)):
                    fn()  # warm

                    def work(fn=fn):
                        for _ in range(40):
                            fn()

                    t0 = time.perf_counter()
                    with ThreadPoolExecutor(max_workers=workers) as pool:
                        list(pool.map(lambda _: work(), range(workers)))
                    elapsed = time.perf_counter() - t0
                    row[label] = (workers * 40) / elapsed
                results["threading"][f"threads_{workers}"] = row
                print(
                    f"| {workers} threads | {row['official']:,.0f} q/s | "
                    f"{row['qql']:,.0f} q/s | {row['qql'] / row['official']:.2f}x |"
                )

        # ------------------------------------------------------- optimize ----
        # Capability check: both sides build/merge segments in-process. Runs
        # after all reads, so index state changes cannot affect earlier numbers.
        if not args.skip_optimize:
            print("\n[optimize]  one-shot segment optimization (post-reads)")
            for label, fn in (
                ("official", lambda: official.optimize(B)),
                ("qql", lambda: qql.optimize(B)),
            ):
                try:
                    t0 = time.perf_counter()
                    changed = fn()
                    elapsed = time.perf_counter() - t0
                    results["optimize"][label] = {
                        "seconds": elapsed,
                        "changed": bool(changed),
                    }
                    print(f"| {label} | {elapsed * 1000:.1f} ms | changed={changed} |")
                except Exception as error:  # noqa: BLE001
                    results["optimize"][label] = {
                        "error": f"{type(error).__name__}: {error}"
                    }
                    print(f"| {label} | ERROR {error} |")

        # ------------------------------------------------------ cold import --
        for module in ("qdrant_edge", "pyqql_edge"):
            reps = 5
            samples = []
            env = os.environ.copy()
            if module == "pyqql_edge":
                env["PYTHONPATH"] = (
                    str(PYQQ_EDGE_PKG) + os.pathsep + env.get("PYTHONPATH", "")
                )
            for _ in range(reps):
                t0 = time.perf_counter()
                subprocess.run(
                    [sys.executable, "-c", f"import {module}"],
                    env=env,
                    check=True,
                    capture_output=True,
                )
                samples.append((time.perf_counter() - t0) * 1000.0)
            median = sorted(samples)[reps // 2]
            results["cold_import_ms"][module] = round(median, 2)
            print(f"cold import {module}: {median:.1f} ms")

        results["loc"] = {
            "official.py": loc(HERE / "official.py"),
            "qql.py": loc(HERE / "qql.py"),
        }
    finally:
        official.close()
        qql.close()

    out = RESULTS / "edge.json"
    out.write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"\nresults -> {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
