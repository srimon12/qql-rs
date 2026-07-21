# /// script
# requires-python = ">=3.11"
# ///

"""QQL Edge Demo — Python edition.

Showcases qdrant-edge (in-process vector search) via the `qql` CLI.
No Qdrant server is required.

Usage:
  # fastembed (local ONNX, no network):
  python main.py

  # HTTP external provider (Ollama, OpenAI, etc.):
  EMBEDDER=http EMBED_URL=http://localhost:11434/v1/embeddings \\
  EMBED_MODEL=nomic-embed-text EMBED_DIM=768 python main.py

  # Dry-run (print QQL statements, do not execute):
  python main.py --dry-run
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

# ── Config ────────────────────────────────────────────────────────────────────

QQL = os.environ.get("QQL_BIN", "/data/codebases/qql-rs/target/debug/qql")
EMBEDDER = os.environ.get("EMBEDDER", "fastembed")
EMBED_URL = os.environ.get("EMBED_URL", "http://localhost:11434/v1/embeddings")
EMBED_KEY = os.environ.get("EMBED_KEY", "")
EMBED_MODEL = os.environ.get("EMBED_MODEL", "nomic-embed-text")
EMBED_DIM = int(os.environ.get("EMBED_DIM", "768"))
DATA_DIR = os.environ.get("EDGE_DATA_DIR", "/tmp/qql-edge-demo-py")
COL = "edge_docs"
DRY_RUN = "--dry-run" in sys.argv

# ── Helpers ───────────────────────────────────────────────────────────────────

def edge(stmt: str) -> dict:
    """Execute a QQL statement against the local qdrant-edge instance."""
    cmd = [QQL, "edge", stmt, "--json", "--data-dir", DATA_DIR, "--embedder", EMBEDDER]
    if EMBEDDER == "http":
        cmd += [
            "--embed-url", EMBED_URL,
            "--embed-key", EMBED_KEY,
            "--embed-model", EMBED_MODEL,
            "--embed-dim", str(EMBED_DIM),
        ]
    if DRY_RUN:
        print(f"  [dry-run] {stmt[:80]}{'...' if len(stmt) > 80 else ''}")
        return {"ok": True, "dry_run": True}
    r = subprocess.run(cmd, capture_output=True, text=True)
    if not r.stdout.strip():
        raise RuntimeError(r.stderr or f"exit {r.returncode}")
    d = json.loads(r.stdout)
    if not d.get("ok"):
        raise RuntimeError(d.get("error") or r.stderr or json.dumps(d))
    return d


def step(label: str, stmt: str, allow_fail: bool = False) -> dict | None:
    print(f"  {label}", end=" ... ", flush=True)
    try:
        d = edge(stmt)
        print("✓")
        return d
    except Exception as e:
        if allow_fail:
            print(f"(skip: {e})")
            return None
        print(f"✗\n    {e}")
        raise


def section(title: str):
    print(f"\n[{title}]")


# ── Demo ──────────────────────────────────────────────────────────────────────

def main() -> None:
    print()
    print("=" * 56)
    print("  QQL Edge Demo")
    print(f"  Embedder : {EMBEDDER}")
    print(f"  Data dir : {DATA_DIR}")
    print(f"  Dry-run  : {DRY_RUN}")
    print("=" * 56)

    # ── 1. Provision ──────────────────────────────────────────────────────────
    section("1. Provision")
    step("Drop (if exists)", f"DROP COLLECTION {COL}", allow_fail=True)
    step("Create HYBRID collection", f"CREATE COLLECTION {COL} HYBRID")
    step("Index: tag (KEYWORD)", f"CREATE INDEX ON COLLECTION {COL} FOR tag TYPE keyword")
    step("Index: year (INTEGER)", f"CREATE INDEX ON COLLECTION {COL} FOR year TYPE integer")

    # ── 2. Seed ───────────────────────────────────────────────────────────────
    section("2. Seed (UPSERT INTO, HYBRID embeddings)")
    DOCS = [
        (1, "Qdrant is a high-performance vector database for AI applications.", "database", 2024),
        (2, "Rust achieves memory safety without a garbage collector.", "systems", 2023),
        (3, "Hybrid search combines dense and sparse retrieval for better recall.", "search", 2024),
        (4, "HNSW is the graph index algorithm used by qdrant-edge.", "database", 2024),
        (5, "Sparse embeddings using BM25 complement dense semantic search.", "search", 2023),
        (6, "qdrant-edge runs in-process with no network hops required.", "database", 2024),
        (7, "The QQL CLI wraps qdrant-edge with a clean query language.", "systems", 2024),
    ]
    for doc_id, text, tag, year in DOCS:
        escaped = text.replace("'", "\\'")
        step(
            f"Upsert #{doc_id} ({tag})",
            f"UPSERT INTO {COL} VALUES {{"
            f"'id': {doc_id}, 'text': '{escaped}', 'tag': '{tag}', 'year': {year}"
            f"}} USING HYBRID",
        )

    # ── 3. Search Modes ───────────────────────────────────────────────────────
    section("3. Search modes")
    QUERY = "vector similarity search"
    modes = [
        ("Dense (semantic)", f"QUERY '{QUERY}' FROM {COL} LIMIT 3"),
        ("Sparse BM25", f"QUERY '{QUERY}' FROM {COL} LIMIT 3 USING SPARSE"),
        ("Hybrid RRF", f"QUERY '{QUERY}' FROM {COL} LIMIT 3 USING HYBRID"),
        ("Hybrid DBSF", f"QUERY '{QUERY}' FROM {COL} LIMIT 3 USING HYBRID FUSION DBSF"),
        ("RRF (k=30, weights)", f"QUERY '{QUERY}' FROM {COL} LIMIT 3 USING HYBRID WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])"),
        ("Exact (brute force)", f"QUERY '{QUERY}' FROM {COL} LIMIT 3 EXACT"),
        ("MMR diversity", f"QUERY '{QUERY}' FROM {COL} LIMIT 5 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 20)"),
    ]
    for label, stmt in modes:
        r = step(label, stmt)
        if r and not DRY_RUN and "data" in r:
            count = len(r["data"])
            print(f"    Found {count} result(s)")

    # ── 4. Filters ────────────────────────────────────────────────────────────
    section("4. Filters")
    filter_cases = [
        ("WHERE tag = 'database'", f"QUERY 'index' FROM {COL} LIMIT 5 WHERE tag = 'database'"),
        ("WHERE year = 2024", f"QUERY 'search' FROM {COL} LIMIT 5 WHERE year = 2024"),
        ("WHERE tag IN ('search','systems')", f"QUERY 'performance' FROM {COL} LIMIT 5 WHERE tag IN ('search', 'systems')"),
        ("Score threshold 0.0", f"QUERY 'qdrant edge' FROM {COL} LIMIT 5 SCORE THRESHOLD 0.0 USING HYBRID"),
        ("Offset pagination", f"QUERY 'vector database' FROM {COL} LIMIT 3 OFFSET 1"),
    ]
    for label, stmt in filter_cases:
        step(label, stmt)

    # ── 5. CTE Prefetch ───────────────────────────────────────────────────────
    section("5. CTE-based Prefetch DAG (no server needed)")
    step(
        "Prefetch RRF (dense + sparse)",
        f"WITH a AS (QUERY 'vector database' USING dense LIMIT 10), "
        f"b AS (QUERY 'vector database' USING sparse LIMIT 10) "
        f"QUERY 'vector database' FROM {COL} LIMIT 3 PREFETCH (a, b) FUSION RRF",
    )
    step(
        "Prefetch RRF with params",
        f"WITH a AS (QUERY 'search' USING dense LIMIT 10), "
        f"b AS (QUERY 'search' USING sparse LIMIT 10) "
        f"QUERY 'search' FROM {COL} LIMIT 3 PREFETCH (a, b) FUSION RRF WITH (rrf_k = 30, rrf_weights = [0.6, 0.4])",
    )

    # ── 6. Recommend ──────────────────────────────────────────────────────────
    section("6. Recommend")
    step("Single positive (id=1)", f"QUERY RECOMMEND WITH (positive = (1)) FROM {COL} LIMIT 3")
    step("Positive + negative", f"QUERY RECOMMEND WITH (positive = (3), negative = (2)) FROM {COL} LIMIT 3")

    # ── 7. Point Access ───────────────────────────────────────────────────────
    section("7. Point access")
    step("SELECT by id", f"SELECT * FROM {COL} WHERE id = 1")
    step("SCROLL all", f"SCROLL FROM {COL} LIMIT 5")
    step("SCROLL filtered", f"SCROLL FROM {COL} WHERE tag = 'database' LIMIT 5")

    # ── 8. Mutations ──────────────────────────────────────────────────────────
    section("8. Mutations")
    step("UPDATE payload", f"UPDATE {COL} SET PAYLOAD = {{'year': 2025}} WHERE id = 2")
    step("DELETE by filter", f"DELETE FROM {COL} WHERE tag = 'systems'")

    # ── 9. Inspect ────────────────────────────────────────────────────────────
    section("9. Inspect")
    info = step("SHOW COLLECTION", f"SHOW COLLECTION {COL}")
    step("SHOW COLLECTIONS", f"SHOW COLLECTIONS")

    # ── Summary ───────────────────────────────────────────────────────────────
    print()
    print("=" * 56)
    if info and not DRY_RUN:
        d = info.get("data", {})
        print(f"  Points   : {d.get('points_count', '?')}")
        print(f"  Topology : {d.get('topology', '?')}")
        print(f"  Quant    : {d.get('quantization', 'none')}")
    print("  Done.")
    print("=" * 56)
    print()


if __name__ == "__main__":
    main()
