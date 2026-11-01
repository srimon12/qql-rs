#!/usr/bin/env python3
"""Client-side sparse BM25: the path for edge, Qdrant < 1.20, and existing
`qdrant/bm25` sparse collections. This works on `dev` today.

QQL encodes BM25 locally (wire-compatible `qdrant/bm25` token IDs) on the
write path and the query path. Tuning knobs (`bm25_k1`, `bm25_b`,
`bm25_avg_len`, language, tokenizer, ...) on the embedder apply ONLY here. They
never affect a Qdrant 1.20 server-side `QUERY BM25` query.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("QQL_LIB", str(_ROOT / "crates" / "pyqql")))

import pyqql  # noqa: E402

COLLECTION = "docs_sparse"

STATEMENTS = [
    f"CREATE COLLECTION {COLLECTION} (sparse SPARSE)",
    f"UPSERT INTO {COLLECTION} VALUES "
    "{id: 1, title: 'The Time Machine'}, "
    "{id: 2, title: 'Back to the Future: a time travel comedy'} "
    "EMBED title INTO sparse USING SPARSE",
    f"QUERY TEXT 'time travel' FROM {COLLECTION} USING sparse PARAMS (idf = 'global') LIMIT 10",
]


def main() -> None:
    for stmt in STATEMENTS:
        ok = pyqql.is_valid(stmt)
        print(f"[{'ok' if ok else 'INVALID'}] {stmt}")
        if ok:
            route = pyqql.compile(stmt)
            print(f"      -> {route.get('method')} {route.get('path')}")


if __name__ == "__main__":
    main()
