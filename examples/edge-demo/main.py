# /// script
# requires-python = ">=3.11"
# ///
"""
QQL Edge Demo — Fully In-Process Vector Search

Showcases qdrant-edge: zero-network, in-process HNSW vector database.
No Qdrant server required. Embeddings via fastembed (local ONNX) or Ollama.

Requirements:
    cargo build --release -p qql-cli
    # (default features include edge + fastembed)

Usage:
    python main.py                          # fastembed (local ONNX, no network)
    python main.py --http                   # Ollama HTTP embedder
    python main.py --dry-run                # print QQL, don't execute
"""

from __future__ import annotations
import json, os, subprocess, sys

QQL = os.environ.get("QQL_BIN", str(__import__("pathlib").Path(__file__).resolve().parent.parent.parent / "target" / "release" / "qql"))
EMBEDDER = "http" if "--http" in sys.argv else "fastembed"
EMBED_URL = os.environ.get("EMBED_URL", "http://localhost:11434/v1/embeddings")
EMBED_KEY = os.environ.get("EMBED_KEY", "")
EMBED_MODEL = os.environ.get("EMBED_MODEL", "nomic-embed-text")
EMBED_DIM = int(os.environ.get("EMBED_DIM", "768"))
DATA_DIR = os.environ.get("EDGE_DATA_DIR", "/tmp/qql-edge-demo")
COL = "edge_docs"
DRY_RUN = "--dry-run" in sys.argv


def edge(stmt: str) -> dict:
    cmd = [QQL, "edge", stmt, "--json", "--data-dir", DATA_DIR, "--embedder", EMBEDDER]
    if EMBEDDER == "http":
        cmd += ["--embed-url", EMBED_URL, "--embed-key", EMBED_KEY,
                "--embed-model", EMBED_MODEL, "--embed-dim", str(EMBED_DIM)]
    if DRY_RUN:
        print(f"    {stmt[:100]}{'...' if len(stmt)>100 else ''}")
        return {"ok": True}

    r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    if not r.stdout.strip():
        msg = r.stderr.strip()
        if "subcommand" in (msg or "").lower() or "unrecognized" in (msg or "").lower():
            raise RuntimeError("Edge not in this binary. Rebuild: cargo build --release -p qql-cli")
        raise RuntimeError(msg or f"exit {r.returncode}")
    d = json.loads(r.stdout)
    if not d.get("ok"):
        err = d.get("error") or r.stderr.strip() or str(d)
        raise RuntimeError(err)
    return d


def step(label: str, stmt: str, allow_fail: bool = False) -> dict | None:
    print(f"  {label}", end=" ... ", flush=True)
    try:
        d = edge(stmt)
        print("✓")
        if not DRY_RUN and "data" in d:
            data = d["data"]
            if isinstance(data, list) and data and "score" in data[0]:
                top = [f"id={h['id']} s={h['score']:.3f}" for h in data[:3]]
                print(f"      {', '.join(top)}")
            elif isinstance(data, dict) and "points_count" in data:
                print(f"      points={data.get('points_count','?')}")
        return d
    except Exception as e:
        if allow_fail:
            print(f"(ok: {e})")
            return None
        print(f"✗\n      {e}")
        raise


def section(title: str):
    print(f"\n{'─'*50}\n  {title}")


def main():
    print()
    print(f"{'='*50}")
    print(f"  QQL Edge Demo")
    print(f"  Embedder: {EMBEDDER}  |  Data: {DATA_DIR}")
    if DRY_RUN:
        print(f"  Mode: dry-run (parsing only)")
    print(f"{'='*50}")

    # ── 1. Schema ──
    section("1. Schema")
    step("Drop (if exists)", f"DROP COLLECTION {COL}", allow_fail=True)
    step("Create HYBRID collection", f"CREATE COLLECTION {COL} HYBRID")
    step("Index tag (keyword)", f"CREATE INDEX ON COLLECTION {COL} FOR tag TYPE keyword")
    step("Index year (integer)", f"CREATE INDEX ON COLLECTION {COL} FOR year TYPE integer")

    # ── 2. Seed (7 records across 4 categories) ──
    section("2. Seed (7 records)")
    DOCS = [
        (1, "Qdrant is a high-performance vector database for AI applications.", "database", 2024),
        (2, "Rust achieves memory safety without a garbage collector.", "systems", 2023),
        (3, "Hybrid search combines dense and sparse retrieval for better recall.", "search", 2024),
        (4, "HNSW is the graph index algorithm used by qdrant-edge for fast ANN.", "database", 2024),
        (5, "Sparse embeddings using BM25 complement dense semantic search.", "search", 2023),
        (6, "qdrant-edge runs entirely in-process with zero network hops.", "database", 2024),
        (7, "QQL compiles to Qdrant wire format with no gateway or sidecar.", "systems", 2024),
    ]
    for doc_id, text, tag, year in DOCS:
        text_escaped = text.replace("'", "\\'")
        step(f"Upsert #{doc_id} ({tag})",
             f"UPSERT INTO {COL} VALUES {{'id': {doc_id}, 'text': '{text_escaped}', 'tag': '{tag}', 'year': {year}}}")

    # ── 3. Search Modes ──
    section("3. Search Modes")
    Q = "vector similarity search"
    step("Dense (semantic)",  f"QUERY '{Q}' FROM {COL} USING dense LIMIT 3")
    step("Sparse BM25",       f"QUERY '{Q}' FROM {COL} USING sparse LIMIT 3")
    step("Hybrid RRF",        f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION RRF FROM {COL} LIMIT 3")
    step("Hybrid DBSF",       f"QUERY HYBRID TEXT '{Q}' DENSE dense SPARSE sparse FUSION DBSF FROM {COL} LIMIT 3")
    step("Exact brute-force", f"QUERY '{Q}' FROM {COL} USING dense PARAMS (exact = true) LIMIT 3")

    # ── 4. Filters + Params ──
    section("4. Filters & Query Params")
    step("WHERE tag = 'database'",   f"QUERY 'index structure' FROM {COL} USING dense WHERE tag = 'database' LIMIT 5")
    step("WHERE year = 2024",        f"QUERY 'search retrieval' FROM {COL} USING dense WHERE year = 2024 LIMIT 5")
    step("WHERE tag IN (...)",       f"QUERY 'performance' FROM {COL} USING dense WHERE tag IN ('search', 'systems') LIMIT 5")
    step("SCORE THRESHOLD 0.0",      f"QUERY 'qdrant edge' FROM {COL} USING dense SCORE THRESHOLD 0.0 LIMIT 5")
    step("OFFSET pagination",        f"QUERY 'vector database' FROM {COL} USING dense LIMIT 3 OFFSET 1")

    # ── 5. CTE Prefetch ──
    section("5. CTE Prefetch DAG")
    step("Prefetch RRF (dense + sparse)",
         f"WITH a AS (QUERY 'vector database' FROM {COL} USING dense LIMIT 10), "
         f"b AS (QUERY 'vector database' FROM {COL} USING sparse LIMIT 10) "
         f"QUERY FUSION RRF FROM {COL} PREFETCH (a, b) LIMIT 3")

    # ── 6. Recommend ──
    section("6. Recommend")
    step("Single positive (id=1)",
         f"QUERY RECOMMEND POSITIVE (1) STRATEGY average_vector FROM {COL} USING dense LIMIT 3")
    step("Positive + negative",
         f"QUERY RECOMMEND POSITIVE (3) NEGATIVE (2) STRATEGY average_vector FROM {COL} USING dense LIMIT 3")

    # ── 7. Point Access ──
    section("7. Point Access")
    step("QUERY POINTS by id",  f"QUERY POINTS (1, 3) FROM {COL} WITH PAYLOAD true")
    step("SCROLL all",          f"SCROLL FROM {COL} LIMIT 5")
    step("SCROLL filtered",     f"SCROLL FROM {COL} WHERE tag = 'database' LIMIT 5")
    step("ORDER BY year DESC",  f"QUERY ORDER BY year DESC FROM {COL} LIMIT 5", allow_fail=True)

    # ── 8. Mutations ──
    section("8. Mutations")
    step("UPDATE payload by id",     f"UPDATE {COL} SET PAYLOAD = {{year: 2025}} WHERE id = 2")
    step("DELETE by filter",         f"DELETE FROM {COL} WHERE tag = 'systems'")

    # ── 9. Operations ──
    section("9. Operations")
    info = step("SHOW COLLECTION", f"SHOW COLLECTION {COL}")
    step("SHOW COLLECTIONS", "SHOW COLLECTIONS")

    # ── Summary ──
    print(f"\n{'='*50}")
    if info and not DRY_RUN:
        d = info.get("data", {})
        print(f"  Points : {d.get('points_count', '?')}")
        print(f"  Topo   : {d.get('topology', '?')}  |  Quant: {d.get('quantization', 'none')}")
    print(f"  Demo complete.")
    print(f"{'='*50}\n")


if __name__ == "__main__":
    main()
