#!/usr/bin/env python3
"""BM25 without embeddings: Qdrant 1.20 server-side BM25 over a full-text index.

DRAFT: uses the proposed QQL syntax (docs/design/bm25-server-side-1.20.md).
Prints "pending QQL grammar" for statements the current parser rejects.

Offline:  python3 examples/qdrant-1.20/bm25_text_search.py
Live:     QDRANT_URL=http://localhost:6333 python3 examples/qdrant-1.20/bm25_text_search.py
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("QQL_LIB", str(_ROOT / "crates" / "pyqql")))

import pyqql  # noqa: E402

COLLECTION = "movies_bm25"

# One constant per statement: swap to Core Rust's final spelling here.
SETUP = [
    # No vectors needed for pure lexical search; a tiny placeholder keeps the
    # collection valid on servers that require a vector config.
    f"CREATE COLLECTION {COLLECTION} (dense VECTOR(4, COSINE))",
    f"CREATE INDEX ON COLLECTION {COLLECTION} FOR title TYPE text WITH (scoring = 'bm25', tokenizer = 'word')",
    f"UPSERT INTO {COLLECTION} VALUES "
    "{id: 1, vector: {dense: [0.1, 0.2, 0.3, 0.4]}, title: 'The Time Machine'}, "
    "{id: 2, vector: {dense: [0.2, 0.1, 0.4, 0.3]}, title: 'Back to the Future: a time travel comedy'}, "
    "{id: 3, vector: {dense: [0.4, 0.3, 0.2, 0.1]}, title: 'Gardening for beginners'}",
]

QUERIES = [
    # Server BM25 with Qdrant defaults (k = 1.2, b = 0.75).
    f"QUERY BM25 'time travel' FROM {COLLECTION} USING title LIMIT 10",
    # Per-query k / b; IDF scoped with the existing PARAMS (idf = …) corpus filter.
    f"QUERY BM25 'time travel' FROM {COLLECTION} USING title "
    "PARAMS (k = 1.5, b = 0.5, idf = 'global') LIMIT 10",
]


def check(stmt: str) -> bool:
    ok = pyqql.is_valid(stmt)
    label = "ok" if ok else "pending QQL grammar"
    print(f"[{label}] {stmt}")
    if ok:
        route = pyqql.compile(stmt)
        print(f"         -> {route.get('method')} {route.get('path')}")
    return ok


def main() -> None:
    print(f"pyqql {getattr(pyqql, '__version__', '?')}: server-side BM25 (Qdrant >= 1.20)\n")
    ready = all([check(s) for s in SETUP + QUERIES])
    url = os.environ.get("QDRANT_URL")
    if not url:
        print("\nOffline only. Set QDRANT_URL to run against Qdrant 1.20.")
        return
    if not ready:
        print("\nSkipping live run: grammar not landed yet.")
        return
    client = pyqql.Client(url, api_key=os.environ.get("QDRANT_API_KEY"))
    for stmt in SETUP:
        client.execute(stmt)
    for stmt in QUERIES:
        print(client.execute(stmt))


if __name__ == "__main__":
    main()
