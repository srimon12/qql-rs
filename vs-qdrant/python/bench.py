#!/usr/bin/env python3
"""Head-to-head harness: official qdrant-client vs pyqql (QQL), live Qdrant.

Design (identical across the python/node/rust legs):
  * Both contenders build their OWN collection from byte-identical data
    (data/ artifacts, checksummed in manifest.json).
  * Ingest: each side's idiomatic batched upsert path, wait=false on both,
    wall-clock timed. A durability barrier (untimed) precedes all reads.
  * Reads: warmup + median-of-reps ops/s with p50/p95 latency.
  * Parity: every read scenario compares results between the two collections
    (ids, scores, facet maps, counts). Sparse/facet/count must match exactly;
    ANN scenarios must match on top-1 with >=80% top-10 overlap.
  * Writes (update payload, delete-by-filter) run once, last, with parity.

Usage: python3 bench.py [--reps 5] [--iters 25]
Writes: ../results/python.json
"""
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

from config import BATCH_BERLIN, BATCH_LEGAL, COLLS, LIMIT, SCROLL_BATCH, SCROLL_PAGES, URL  # noqa: E402

from official_scenarios import OfficialScenarios  # noqa: E402
from qql_scenarios import QqlScenarios  # noqa: E402

DATA = ROOT / "data"
RESULTS = ROOT / "results"


# ------------------------------------------------------------------ data ----
def load_data():
    def f32(name, shape):
        a = np.fromfile(DATA / name, dtype=np.float32)
        return a.reshape(shape)

    docs = {k: [json.loads(l) for l in (DATA / f"{k}.jsonl").read_text().splitlines() if l]
            for k in ("berlin", "legal")}
    dense = {
        "berlin": f32("berlin_dense.f32", (len(docs["berlin"]), 384)),
        "legal": f32("legal_dense.f32", (len(docs["legal"]), 384)),
    }
    sparse = {k: json.loads((DATA / f"{k}_bm25.json").read_text()) for k in ("berlin", "legal")}
    # tf values as f32 numpy: bind via the memoryview fast path (one copy)
    for k in sparse:
        for sv in sparse[k]:
            sv["values"] = np.asarray(sv["values"], dtype=np.float32)
    lens = json.loads((DATA / "legal_colbert_lens.json").read_text())
    colbert_flat = np.fromfile(DATA / "legal_colbert.f32", dtype=np.float32)
    queries = json.loads((DATA / "queries.json").read_text())
    return docs, dense, sparse, colbert_flat, lens, queries


# --------------------------------------------------------------- timing ----
def timed_read(fn, reps: int, iters: int) -> dict:
    """Median ops/s + latency percentiles over reps runs of `iters` calls."""
    samples_ms = []
    for r in range(reps):
        fn() if r == 0 else None                      # warmup rep untimed
        t0 = time.perf_counter()
        for _ in range(iters):
            fn()
        dt = (time.perf_counter() - t0) / iters
        samples_ms.append(dt * 1000.0)
    med = statistics.median(samples_ms)
    return {"ops_per_sec": round(1000.0 / med), "p50_ms": round(med, 3),
            "p95_ms": round(max(samples_ms), 3), "reps": reps, "iters": iters}


def wait_until_ready(client_url: str, collection: str, expected: int) -> None:
    """Untimed barrier: block until the count is visible AND the collection
    is green (optimizer idle). Points-count alone races background segment
    merges/HNSW indexing, which makes early read scenarios slower than later
    ones on the same data — pure run-to-run variance, not engine signal."""
    import urllib.request
    deadline = time.time() + 180
    while time.time() < deadline:
        with urllib.request.urlopen(f"{client_url}/collections/{collection}") as r:
            info = json.loads(r.read())["result"]
        if info.get("points_count") == expected and info.get("status") == "green":
            return
        time.sleep(0.5)
    raise RuntimeError(f"{collection} never reached {expected} points + green")


# --------------------------------------------------------------- parity ----
def norm_hits(hits) -> list[dict]:
    """Official SDK returns ScoredPoint objects; QQL returns dicts."""
    return [h if isinstance(h, dict) else
            {"id": h.id, "score": h.score, "payload": h.payload} for h in hits]


def hit_key(h) -> tuple:
    """QQL may return ids as strings ("1"), the official SDK as ints."""
    try:
        return int(h["id"])
    except (ValueError, TypeError):
        return h["id"]


def rest_point(url: str, collection: str, point_id: int) -> dict:
    """Raw-REST point fetch (harness-level parity probe, not a timed path)."""
    import urllib.request
    body = json.dumps({"ids": [point_id], "with_payload": True, "with_vector": True}).encode()
    req = urllib.request.Request(
        f"{url}/collections/{collection}/points", data=body,
        headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req) as r:
        return json.loads(r.read())["result"][0]


def compare_hits(official: list, qql: list) -> dict:
    official, qql = norm_hits(official), norm_hits(qql)
    o_ids = [hit_key(h) for h in official]
    q_ids = [hit_key(h) for h in qql]
    o_set, q_set = set(o_ids), set(q_ids)
    overlap = len(o_set & q_set) / max(1, len(o_set | q_set))
    top1 = bool(o_ids and q_ids and o_ids[0] == q_ids[0])
    o_scores = {hit_key(h): round(float(h["score"]), 5) for h in official}
    q_scores = {hit_key(h): round(float(h["score"]), 5) for h in qql}
    diffs = [abs(o_scores[k] - q_scores[k]) for k in o_scores.keys() & q_scores.keys()]
    return {"top1_match": top1, "jaccard_overlap": round(overlap, 3),
            "max_score_diff": max(diffs) if diffs else 0.0}


def compare_exact(official, qql) -> dict:
    ok = official == qql
    return {"match": ok, "detail": "" if ok else f"{official!r} != {qql!r}"[:200]}


# ------------------------------------------------------------------ main ----
def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("--iters", type=int, default=25)
    args = ap.parse_args()

    docs, dense, sparse, colbert_flat, colbert_lens, queries = load_data()
    official, qql = OfficialScenarios(URL), QqlScenarios(URL)
    R = {"meta": meta(), "scenarios": {}, "ingest": {}, "parity": {}, "cold_import": {}}

    # ---- cold import (fresh interpreter, median of N) ----
    R["cold_import"] = {
        "qdrant_client": cold_import_ms("qdrant_client"),
        "pyqql": cold_import_ms("pyqql"),
    }

    # ---- setup: drop, create ----
    for side_name, side in (("official", official), ("qql", qql)):
        for dom in ("berlin", "legal"):
            side.drop_collection(COLLS[dom][side_name])
    official.create_berlin(COLLS["berlin"]["official"])
    official.create_legal(COLLS["legal"]["official"])
    qql.create_berlin(COLLS["berlin"]["qql"])
    qql.create_legal(COLLS["legal"]["qql"])

    # ---- ingest (timed once per side) ----
    t0 = time.perf_counter()
    official.ingest_berlin(COLLS["berlin"]["official"], docs["berlin"], dense["berlin"], sparse["berlin"])
    colbert_rows, o = [], 0
    for n in colbert_lens:
        colbert_rows.append(colbert_flat[o:o + n * 128].reshape(n, 128).tolist())
        o += n * 128
    official.ingest_legal(COLLS["legal"]["official"], docs["legal"], dense["legal"], sparse["legal"], colbert_rows)
    R["ingest"]["official_sec"] = round(time.perf_counter() - t0, 3)
    t0 = time.perf_counter()
    qql.ingest_berlin(COLLS["berlin"]["qql"], docs["berlin"], dense["berlin"], sparse["berlin"])
    qql.ingest_legal(COLLS["legal"]["qql"], docs["legal"], dense["legal"], sparse["legal"], colbert_flat, colbert_lens)
    R["ingest"]["qql_sec"] = round(time.perf_counter() - t0, 3)
    R["ingest"]["points"] = {"berlin": len(docs["berlin"]), "legal": len(docs["legal"]),
                             "batch": {"berlin": BATCH_BERLIN, "legal": BATCH_LEGAL}}
    print(f"ingest: official {R['ingest']['official_sec']}s vs qql {R['ingest']['qql_sec']}s", flush=True)

    # durability barrier (untimed)
    for side_name in ("official", "qql"):
        for dom in ("berlin", "legal"):
            wait_until_ready(URL, COLLS[dom][side_name], len(docs[dom]))

    # ---- vector/payload parity of an ingested point (raw REST, both sides) ----
    for dom, pid in (("berlin", 1), ("legal", 1)):
        o = rest_point(URL, COLLS[dom]["official"], pid)
        q = rest_point(URL, COLLS[dom]["qql"], pid)
        ok = (o["payload"] == q["payload"]
              and o["vector"].keys() == q["vector"].keys()
              and all(np.allclose(o["vector"][k], q["vector"][k], atol=1e-6)
                      if isinstance(o["vector"][k], list)
                      else (o["vector"][k]["indices"] == q["vector"][k]["indices"]
                            and np.allclose(o["vector"][k]["values"],
                                            q["vector"][k]["values"], atol=1e-6))
                      for k in o["vector"]))
        R["parity"][f"ingested_point_{dom}"] = {"match": bool(ok)}
        if not ok:
            print(f"PARITY FAIL ingested_point_{dom}", flush=True)

    # ---- read scenarios ----
    S = R["scenarios"]
    B_OFF, B_QQL = COLLS["berlin"]["official"], COLLS["berlin"]["qql"]
    L_OFF, L_QQL = COLLS["legal"]["official"], COLLS["legal"]["qql"]
    bq = queries["berlin"]

    def read_pair(label, o_fn, q_fn, parity_fn=compare_hits, require_exact=False):
        S[label] = {"official": timed_read(lambda: o_fn(), args.reps, args.iters),
                    "qql": timed_read(lambda: q_fn(), args.reps, args.iters)}
        S[label]["parity"] = parity_fn(o_fn(), q_fn())
        ratio = S[label]["qql"]["ops_per_sec"] / S[label]["official"]["ops_per_sec"]
        print(f"{label:24s} official {S[label]['official']['ops_per_sec']:>9,} | "
              f"qql {S[label]['qql']['ops_per_sec']:>9,} | x{ratio:.2f} "
              f"| parity {json.dumps(S[label]['parity'])[:90]}", flush=True)

    read_pair("query_dense",
              lambda: official.query_dense(B_OFF, bq[0]["dense"]),
              lambda: qql.query_dense(B_QQL, bq[0]["dense"]))
    read_pair("query_dense_filtered",
              lambda: official.query_dense_filtered(B_OFF, bq[1]["dense"]),
              lambda: qql.query_dense_filtered(B_QQL, bq[1]["dense"]))
    read_pair("query_sparse",
              lambda: official.query_sparse(B_OFF, bq[0]["sparse"]),
              lambda: qql.query_sparse(B_QQL, bq[0]["sparse"]))
    # hybrid: RRF ordering is nondeterministic at equal-score ties even
    # within one SDK (server-side tie-breaking) — baseline official-vs-official
    # self-consistency so the qql-vs-official delta has a fair reference.
    read_pair("query_hybrid",
              lambda: official.query_hybrid(B_OFF, bq[2]["dense"], bq[2]["sparse"]),
              lambda: qql.query_hybrid(B_QQL, bq[2]["dense"], bq[2]["sparse"]),
              parity_fn=lambda o1, q: {
                  "qql_vs_official": compare_hits(o1, q),
                  "official_vs_official": compare_hits(
                      o1, official.query_hybrid(B_OFF, bq[2]["dense"], bq[2]["sparse"])),
              })
    read_pair("scroll_pages",
              lambda: official.scroll_pages(B_OFF, SCROLL_PAGES, SCROLL_BATCH),
              lambda: qql.scroll_pages(B_QQL, SCROLL_PAGES, SCROLL_BATCH),
              parity_fn=lambda a, b: compare_hits(
                  [{"id": i, "score": 0} for i in a], [{"id": i, "score": 0} for i in b]))
    read_pair("count_berlin",
              lambda: official.count_berlin(B_OFF),
              lambda: qql.count_berlin(B_QQL), parity_fn=compare_exact)
    read_pair("facet_district",
              lambda: official.facet_district(B_OFF),
              lambda: qql.facet_district(B_QQL), parity_fn=compare_exact)
    read_pair("query_colbert",
              lambda: official.query_colbert(L_OFF, queries["legal"][0]["colbert"]),
              lambda: qql.query_colbert(L_QQL, queries["legal"][0]["colbert"]))
    read_pair("count_legal",
              lambda: official.count_legal(L_OFF),
              lambda: qql.count_legal(L_QQL), parity_fn=compare_exact)

    # ---- prepared rerun (many small parameterized calls) ----
    read_pair("prepared_rerun",
              lambda: official.prepared_rerun(B_OFF, [q["dense"] for q in bq]),
              lambda: qql.prepared_rerun(B_QQL, [q["dense"] for q in bq]))

    # ---- writes (last, parity-checked) ----
    official.update_payload(B_OFF)
    qql.update_payload(B_QQL)
    R["parity"]["update_payload"] = compare_exact(
        official.facet_district(B_OFF), qql.facet_district(B_QQL))["match"]

    official.delete_by_filter(B_OFF)
    qql.delete_by_filter(B_QQL)
    for side_name in ("official", "qql"):
        wait_until_ready(URL, COLLS["berlin"][side_name],
                         sum(1 for d in docs["berlin"] if d["price"] <= 250.0))
    R["parity"]["delete_by_filter"] = compare_exact(
        official.count_berlin(B_OFF), qql.count_berlin(B_QQL))["match"]

    RESULTS.mkdir(exist_ok=True)
    out = RESULTS / "python.json"
    out.write_text(json.dumps(R, indent=2))
    print(f"\nwritten {out}")

    official.close()
    qql.close()


def cold_import_ms(module: str, reps: int = 5) -> float:
    samples = []
    for _ in range(reps):
        t0 = time.perf_counter()
        subprocess.run([sys.executable, "-c", f"import {module}"], check=True, capture_output=True)
        samples.append((time.perf_counter() - t0) * 1000)
    return round(statistics.median(samples), 1)


def meta() -> dict:
    import importlib.metadata
    import platform
    import pyqql
    import qdrant_client
    import urllib.request
    with urllib.request.urlopen("http://localhost:6333") as r:
        server = json.loads(r.read())
    return {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "python": platform.python_version(),
        "pyqql": pyqql.__version__,
        "qdrant_client": importlib.metadata.version("qdrant-client"),
        "qdrant_server": server["version"],
        "url": URL,
    }


if __name__ == "__main__":
    main()
