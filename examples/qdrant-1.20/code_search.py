#!/usr/bin/env python3
"""Code search on Qdrant 1.20: `code` tokenizer + BM25 + SUBSTRING path filter.

DRAFT: proposed QQL syntax (docs/design/bm25-server-side-1.20.md). The
`code` tokenizer splits identifiers such as `parseHttpRequest` and
`parse_http_request` into searchable parts.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("QQL_LIB", str(_ROOT / "crates" / "pyqql")))

import pyqql  # noqa: E402

COLLECTION = "repo_code"

STATEMENTS = [
    f"CREATE COLLECTION {COLLECTION} (dense VECTOR(4, COSINE))",
    f"CREATE INDEX ON COLLECTION {COLLECTION} FOR body TYPE text WITH (scoring = 'bm25', tokenizer = 'code')",
    # `prefix = true` keyword index serves SUBSTRING from its key dictionary.
    f"CREATE INDEX ON COLLECTION {COLLECTION} FOR path TYPE keyword WITH (prefix = true)",
    f"QUERY BM25 'http request parser' FROM {COLLECTION} USING body "
    "WHERE path SUBSTRING 'src/' LIMIT 10",
]


def main() -> None:
    for stmt in STATEMENTS:
        label = "ok" if pyqql.is_valid(stmt) else "pending QQL grammar"
        print(f"[{label}] {stmt}")


if __name__ == "__main__":
    main()
