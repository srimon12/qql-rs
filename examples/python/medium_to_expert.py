#!/usr/bin/env python3
"""
Medium → Expert (Python / pyqql)

Multi-tenant gateway:
  - inject_filter for logical isolation (always)
  - SHARD 'tenant' in QQL (or stmt.shard_key after parse) for routing
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("QQL_LIB", str(_ROOT / "crates" / "pyqql")))

import pyqql  # noqa: E402

USERS = {
    "alice": {"tenant": "acme", "role": "admin"},
    "bob": {"tenant": "acme", "role": "viewer"},
    "charlie": {"tenant": "globex", "role": "viewer"},
}


def secure(query: str, user: str) -> pyqql.Stmt:
    ctx = USERS[user]
    # Prefer SHARD in the source when tenant is known at template time.
    # Here we demonstrate host property after parse (auth-resolved tenant).
    stmt = pyqql.parse(query)[0]
    stmt.inject_filter("tenant_id", "=", ctx["tenant"])
    stmt.shard_key = ctx["tenant"]
    if ctx["role"] == "viewer":
        stmt.inject_filter("status", "=", "published")
    return stmt


STRATEGIES = {
    "hybrid": """
        QUERY TEXT '{q}'
        FROM knowledge_base
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        LIMIT 5
    """,
    "multistage": """
        WITH
          dense AS (
            QUERY TEXT '{q}' FROM knowledge_base USING dense LIMIT 100
          ),
          sparse AS (
            QUERY TEXT '{q}' FROM knowledge_base USING sparse LIMIT 100
          )
        QUERY FUSION RRF FROM knowledge_base
        PREFETCH (dense, sparse)
        LIMIT 5
    """,
    "formula": """
        WITH candidates AS (
          QUERY TEXT '{q}' FROM knowledge_base USING dense LIMIT 50
        )
        QUERY FORMULA (score * 0.7 + popularity * 0.3) DEFAULTS (popularity = 0.0)
        FROM knowledge_base
        PREFETCH (candidates)
        LIMIT 5
    """,
}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--live", action="store_true")
    ap.add_argument("--user", default="bob", choices=list(USERS))
    ap.add_argument("--strategy", default="hybrid", choices=list(STRATEGIES))
    ap.add_argument("--query", default="acute myocardial infarction")
    args = ap.parse_args()

    print(f"pyqql {getattr(pyqql, '__version__', '?')}")
    print(f"user={args.user} tenant={USERS[args.user]['tenant']}\n")

    raw = STRATEGIES[args.strategy].format(q=args.query.replace("'", "\\'"))
    print("── raw QQL ──\n", raw.strip(), "\n", sep="")

    secured = secure(raw, args.user)
    print("── secured ──")
    print(f"  shard_key = {secured.shard_key!r}")
    print(f"  filter    = {json.dumps((secured.to_dict().get('Query') or {}).get('filter'))[:200]}")
    print()

    # Same query with SHARD written in SQL (preferred when authoring templates)
    literal = f"""
        QUERY TEXT '{args.query.replace(chr(39), chr(92)+chr(39))}'
        FROM knowledge_base
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        SHARD '{USERS[args.user]["tenant"]}'
        LIMIT 5
    """
    lit = pyqql.parse(literal)[0]
    print("── SHARD in QQL ──")
    print(f"  is_valid={pyqql.is_valid(literal)}  shard_key={lit.shard_key!r}")
    print()

    for name, tmpl in STRATEGIES.items():
        q = tmpl.format(q="demo")
        s = secure(q, args.user)
        print(f"  {name:12s} valid={pyqql.is_valid(q)} shard={s.shard_key!r}")

    # ── Qdrant 1.19 / QQL 1.4 surface (offline parse/plan checks) ──
    q19 = [
        "CREATE COLLECTION docs (dense VECTOR(384, COSINE) "
        "WITH VECTOR (memory = 'cached', datatype = 'turbo4')) "
        "WITH HNSW (memory = 'cold') WITH PARAMS (payload_memory = 'cold')",
        "CREATE INDEX ON COLLECTION docs FOR title TYPE keyword "
        "WITH (prefix = true)",
        "QUERY TEXT 'compliance' FROM docs USING dense "
        "WHERE title MATCH PREFIX 'Comp' AND SLICE (4, 0) LIMIT 20",
        "QUERY TEXT 'risks' FROM docs USING sparse "
        "PARAMS (idf = 'global') LIMIT 10",
        "SHOW QUOTAS",
        "SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true",
    ]
    print("── Qdrant 1.19 surface (offline) ──")
    for s in q19:
        route = pyqql.compile_query(s)
        print(f"  valid={pyqql.is_valid(s)}  {route.get('method')} {route.get('path')}")
    print()

    if not args.live:
        print("\nOffline complete.")
        return 0

    # Qdrant 1.19 read affinity: pins reads to a stable replica via
    # X-Qdrant-Route-Affinity (REST header) / gRPC metadata. Transport only —
    # empty strings are treated as unset. Not available on edge (single node).
    client = pyqql.Client(
        "http://localhost:6333",
        route_affinity=f"session-{args.user}",
        embedder=pyqql.HttpEmbedder(
            "http://localhost:11434/v1/embeddings", "all-minilm:l6-v2", 384
        ),
    )
    print(f"── live route_affinity={client.route_affinity!r} ──")
    try:
        print(json.dumps(client.execute(secured), indent=2)[:500])
    except Exception as e:
        print(f"  live failed: {e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
