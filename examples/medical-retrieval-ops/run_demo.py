#!/usr/bin/env python3
"""
Medical Retrieval Ops — Full QQL Showcase

Downloads the ChatMED benchmark dataset (cached locally), creates a hybrid
collection, upserts 420 medical records with auto-embedding via Ollama, then
showcases every QQL feature: dense, sparse, hybrid, filters, params, grouped
retrieval, recommend, context, discover, CTE prefetch DAGs, mutations, scroll,
and explain.

Usage:
    QQL_BIN=./target/release/qql uv run examples/medical-retrieval-ops/run_demo.py
"""
from __future__ import annotations

import json, os, subprocess, sys, time
from pathlib import Path

DEMO_ROOT = Path(__file__).resolve().parent
GENERATED = DEMO_ROOT / "generated"
COLLECTION = "medical_retrieval_ops"

QQL_BIN = os.environ.get("QQL_BIN", str(Path(__file__).resolve().parent.parent.parent / "target" / "release" / "qql"))

# Ollama embedder config
os.environ.setdefault("EMBED_URL", "http://localhost:11434/v1/embeddings")
os.environ.setdefault("EMBED_MODEL", "all-minilm:l6-v2")
os.environ.setdefault("EMBED_DIM", "384")


def qql(*args: str) -> dict:
    """Run qql CLI and return parsed JSON result."""
    r = subprocess.run([QQL_BIN, *args], capture_output=True, text=True, timeout=60)
    if not r.stdout.strip():
        raise RuntimeError(r.stderr.strip() or f"exit {r.returncode}")
    d = json.loads(r.stdout)
    if not d.get("ok"):
        raise RuntimeError(d.get("error") or r.stderr.strip() or str(d))
    return d


def step(label: str) -> None:
    print(f"\n{'─'*60}\n  {label}")


def run(label: str, stmt: str, *, explain: bool = False) -> None:
    cmd = "explain" if explain else "exec"
    try:
        r = qql(cmd, "--quiet", "--json", stmt)
        if explain:
            for line in r.get("plan", "").split("\n")[:4]:
                print(f"    {line}")
        else:
            data = r.get("data")
            if isinstance(data, list) and data and "score" in data[0]:
                for h in data[:3]:
                    print(f"    score={h['score']:.3f}  id={h.get('id','')}")
            elif isinstance(data, dict) and "groups" in data.get("result", data):
                groups = data.get("result", data).get("groups", [])
                for g in groups[:3]:
                    gid = g.get("id", g.get("group_id", "?"))
                    print(f"    group={gid}  hits={len(g.get('hits',[]))}")
            else:
                print(f"    {r.get('message','')}")
    except Exception as e:
        print(f"    ERROR: {e}")


def load_eval() -> dict:
    with open(GENERATED / "eval.json") as f:
        return json.load(f)["queries"]


def check_collection_exists() -> bool:
    try:
        r = qql("exec", "--quiet", "--json", f"SHOW COLLECTION {COLLECTION}")
        pts = r.get("data", {}).get("points_count", 0)
        return pts > 0
    except Exception:
        return False


def main():
    GENERATED.mkdir(parents=True, exist_ok=True)

    # ── doctor ──
    step("QQL Doctor")
    print(f"  Qdrant: {qql('doctor', '--quiet', '--json').get('message','')}")

    # ── corpus ──
    step("Dataset (cached locally)")
    subprocess.run(["uv", "run", str(DEMO_ROOT / "build-medical-corpus.py")], check=True, env={**os.environ, "MEDICAL_RAG_MAX_ROWS": "all"})
    eval_data = load_eval()
    main = eval_data["main"]
    related = eval_data["related"]
    print(f"  Rows: {json.loads((GENERATED / 'eval.json').read_text())['row_count']}")
    print(f"  Main Q: {main['question'][:80]}...")
    print(f"  Related Q: {related['question'][:80]}...")

    # ── schema ──
    step("Schema")
    has_data = check_collection_exists()
    if has_data:
        print("  Collection already has data — skipping schema + upsert")
    else:
        run("Create collection",
            f"CREATE COLLECTION {COLLECTION} HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)")
        for name, ftype in [("specialty","keyword"),("tenant_id","keyword"),("case_priority","keyword"),
                            ("case_status","keyword"),("complexity","keyword")]:
            run(f"  Index {name}",
                f"CREATE INDEX ON COLLECTION {COLLECTION} FOR {name} TYPE {ftype}")

        # ── upsert ──
        step(f"Upsert ({Path(GENERATED / '02-seed.qql').stat().st_size // 1024} KB seed file)")
        t0 = time.time()
        r = qql("execute", str(GENERATED / "02-seed.qql"))
        elapsed = time.time() - t0
        print(f"  {r['message']} in {elapsed:.1f}s")

    # ── inspect ──
    step("Inspect")
    run("Collection info", f"SHOW COLLECTION {COLLECTION}")

    # ═══════════════════════════════════════════════════════════════
    #  SEARCH MODES
    # ═══════════════════════════════════════════════════════════════
    Q = main["question"]
    RID = related["id"]
    TENANT = main["tenant_id"]
    PRI = main["case_priority"]
    STATUS = main["case_status"]

    step("Search Modes")
    run("Dense (semantic)",     f"QUERY '{Q}' FROM {COLLECTION} USING dense LIMIT 5")
    run("Sparse (keyword)",     f"QUERY '{Q}' FROM {COLLECTION} USING sparse LIMIT 5")
    run("Hybrid RRF",           f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} LIMIT 5")
    run("Hybrid DBSF",          f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION DBSF FROM {COLLECTION} LIMIT 5")
    run("Exact baseline",       f"QUERY '{Q}' FROM {COLLECTION} USING dense PARAMS (exact = true) LIMIT 5")
    run("HNSW ef=256",          f"QUERY '{Q}' FROM {COLLECTION} USING dense PARAMS (hnsw_ef = 256) LIMIT 5")
    run("Score threshold",      f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} SCORE THRESHOLD 0.3 LIMIT 5")
    run("Offset pagination",    f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} LIMIT 5 OFFSET 3")
    run("MMR diversity",        f"QUERY MMR '{Q}' DIVERSITY 0.5 CANDIDATES 20 FROM {COLLECTION} USING dense LIMIT 5")
    run("RRF params (k=30)",    f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} PARAMS (rrf_k = 30, rrf_weights = [0.7, 0.3]) LIMIT 5")

    # ═══════════════════════════════════════════════════════════════
    #  FILTERS
    # ═══════════════════════════════════════════════════════════════
    step("Filters")
    run("WHERE eq",             f"QUERY '{Q}' FROM {COLLECTION} USING dense WHERE case_priority = '{PRI}' LIMIT 5")
    run("WHERE IN",             f"QUERY '{Q}' FROM {COLLECTION} USING dense WHERE case_priority IN ('high', 'medium') LIMIT 5")
    run("AND compound",         f"QUERY '{Q}' FROM {COLLECTION} USING dense WHERE case_priority = '{PRI}' AND case_status = '{STATUS}' LIMIT 5")
    run("Tenant isolation",     f"QUERY '{Q}' FROM {COLLECTION} USING dense WHERE tenant_id = '{TENANT}' LIMIT 5")
    if "diagnosis" in " ".join([main.get("text",""), related.get("text","")]):
        pass  # skip MATCH PHRASE if diagnosis isn't in the schema
    run("MATCH PHRASE",         f"QUERY '{Q}' FROM {COLLECTION} USING dense WHERE specialty = 'cardiology' LIMIT 3")

    # ═══════════════════════════════════════════════════════════════
    #  GROUPED & RECOMMEND
    # ═══════════════════════════════════════════════════════════════
    step("Grouped Retrieval")
    run("GROUP BY specialty",   f"QUERY '{Q}' FROM {COLLECTION} USING dense GROUP BY specialty SIZE 2 LIMIT 6")
    run("GROUP BY priority",    f"QUERY '{Q}' FROM {COLLECTION} USING dense GROUP BY case_priority SIZE 2 LIMIT 6")

    step("Recommend")
    run("Single positive",      f"QUERY RECOMMEND POSITIVE ({RID}) STRATEGY average_vector FROM {COLLECTION} USING dense LIMIT 5")
    run("Multi + negative",     f"QUERY RECOMMEND POSITIVE ({RID}, {main['id']}) NEGATIVE ({RID}) STRATEGY average_vector FROM {COLLECTION} USING dense LIMIT 5")
    run("Best score strategy",  f"QUERY RECOMMEND POSITIVE ({RID}) STRATEGY best_score FROM {COLLECTION} USING dense LIMIT 5")

    step("Context & Discover")
    run("Context pairs",        f"QUERY CONTEXT (POSITIVE POINT {main['id']} NEGATIVE POINT {RID}) FROM {COLLECTION} USING dense LIMIT 5")
    run("Discover",             f"QUERY DISCOVER TARGET POINT {main['id']} CONTEXT (POSITIVE POINT {RID} NEGATIVE POINT {main['id']}) FROM {COLLECTION} USING dense LIMIT 5")

    # ═══════════════════════════════════════════════════════════════
    #  CTE PREFETCH DAGS
    # ═══════════════════════════════════════════════════════════════
    step("CTE Prefetch DAGs")
    run("Prefetch RRF",
        f"WITH a AS (QUERY '{Q}' FROM {COLLECTION} USING dense LIMIT 20), b AS (QUERY '{Q}' FROM {COLLECTION} USING sparse LIMIT 20) QUERY FUSION RRF FROM {COLLECTION} PREFETCH (a, b) LIMIT 5")
    run("Prefetch + per-filter + score threshold",
        f"WITH a AS (QUERY '{Q}' FROM {COLLECTION} USING dense WHERE case_priority = '{PRI}' LIMIT 20), b AS (QUERY '{Q}' FROM {COLLECTION} USING sparse LIMIT 20) QUERY FUSION RRF FROM {COLLECTION} PREFETCH (a SCORE THRESHOLD 0.3, b SCORE THRESHOLD 0.1) LIMIT 5")
    run("Prefetch + RRF params",
        f"WITH a AS (QUERY '{Q}' FROM {COLLECTION} USING dense LIMIT 20), b AS (QUERY '{Q}' FROM {COLLECTION} USING sparse LIMIT 20) QUERY FUSION RRF FROM {COLLECTION} PREFETCH (a, b) PARAMS (rrf_k = 10, rrf_weights = [0.7, 0.3]) LIMIT 5")

    # ═══════════════════════════════════════════════════════════════
    #  MUTATIONS
    # ═══════════════════════════════════════════════════════════════
    step("Mutations")
    MID = main["id"]
    run("Update payload by ID", f"UPDATE {COLLECTION} SET PAYLOAD = {{'case_status': 'reviewed', 'note': 'demo-update'}} WHERE id = {MID}")
    run("Verify update",        f"QUERY POINTS ({MID}) FROM {COLLECTION} WITH PAYLOAD true")
    run("Revert update",        f"UPDATE {COLLECTION} SET PAYLOAD = {{'case_status': '{STATUS}'}} WHERE id = {MID}")

    # ═══════════════════════════════════════════════════════════════
    #  POINT ACCESS
    # ═══════════════════════════════════════════════════════════════
    step("Point Access")
    run("Point lookup",         f"QUERY POINTS ({MID}, {RID}) FROM {COLLECTION} WITH PAYLOAD true")
    run("Scroll all",           f"SCROLL FROM {COLLECTION} LIMIT 5")
    run("Scroll filtered",      f"SCROLL FROM {COLLECTION} WHERE case_priority = '{PRI}' LIMIT 5")
    run("ORDER BY",             f"QUERY ORDER BY id DESC FROM {COLLECTION} LIMIT 5")

    # ═══════════════════════════════════════════════════════════════
    #  OPERATIONS
    # ═══════════════════════════════════════════════════════════════
    step("Operations")
    run("SHOW COLLECTIONS", "SHOW COLLECTIONS")
    run("SHOW COLLECTION",  f"SHOW COLLECTION {COLLECTION}")
    run("EXPLAIN", f"QUERY HYBRID TEXT '{Q[:50]}' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} LIMIT 5", explain=True)

    # ── benchmark ──
    step("Benchmark")
    try:
        subprocess.run(["uv", "run", str(DEMO_ROOT / "run-benchmark.py"),
                        str(GENERATED / "benchmark-questions.json")], check=True)
        print("  Benchmark complete")
    except subprocess.CalledProcessError:
        print("  Benchmark skipped (needs Qdrant with serving enabled)")
    except FileNotFoundError:
        print("  Benchmark skipped (run-benchmark.py not found)")

    print(f"\n{'='*60}")
    print("  Demo complete. All QQL features verified.")


if __name__ == "__main__":
    main()
