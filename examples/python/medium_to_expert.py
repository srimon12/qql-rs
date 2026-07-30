#!/usr/bin/env python3
"""
Medium → Expert (Python / pyqql)

Multi-tenant security gateway pattern:
  - inject_filter  for logical isolation (WHERE tenant_id = …)
  - inject_shard_key for physical routing (SHARD '…')
  - role-based extra filters (viewers cannot see confidential rows)
  - hybrid / formula / CTE strategies as pure QQL strings

This script is offline-first (parse + inject + explain). Pass --live
to attempt execution against a local Qdrant + optional Ollama embedder.
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


# ── Tenant / role table (pretend auth context) ───────────────────────
USERS = {
    "alice":   {"tenant": "acme",   "role": "admin"},
    "bob":     {"tenant": "acme",   "role": "viewer"},
    "charlie": {"tenant": "globex", "role": "viewer"},
}


def secure(query: str, user: str) -> pyqql.Stmt:
    """Single call site: every query is tenant-isolated before it leaves the host."""
    ctx = USERS[user]
    stmt = pyqql.parse(query)[0]
    # Layer 2 — payload filter (always)
    stmt.inject_filter("tenant_id", "=", ctx["tenant"])
    # Layer 1 — physical shard (always)
    stmt.inject_shard_key(ctx["tenant"])
    # Role gate — viewers only see published content
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
    ap = argparse.ArgumentParser(description="QQL multi-tenant gateway example")
    ap.add_argument("--live", action="store_true", help="Execute against localhost:6333")
    ap.add_argument("--user", default="bob", choices=list(USERS))
    ap.add_argument("--strategy", default="hybrid", choices=list(STRATEGIES))
    ap.add_argument("--query", default="acute myocardial infarction")
    args = ap.parse_args()

    print(f"pyqql {getattr(pyqql, '__version__', '?')}")
    print(f"user={args.user}  tenant={USERS[args.user]['tenant']}  role={USERS[args.user]['role']}")
    print(f"strategy={args.strategy}\n")

    raw = STRATEGIES[args.strategy].format(q=args.query.replace("'", "\\'"))
    print("── raw QQL (unsecured) ──")
    print(raw.strip())
    print()

    secured = secure(raw, args.user)
    print("── after inject_filter + inject_shard_key ──")
    print(f"  shard_key = {secured.shard_key!r}")
    qdict = secured.to_dict().get("Query") or secured.to_dict()
    filt = qdict.get("filter") if isinstance(qdict, dict) else None
    print(f"  filter    = {json.dumps(filt)[:200] if filt else '(nested / CTE)'}")
    print()

    print("── explain (auditable base plan) ──")
    plan_src = STRATEGIES[args.strategy].format(q=args.query.replace("'", "\\'"))
    print(pyqql.explain(plan_src).get("plan", ""))
    print()

    # Demonstrate all three strategies parse cleanly
    print("── strategy inventory ──")
    for name, tmpl in STRATEGIES.items():
        q = tmpl.format(q="demo query")
        ok = pyqql.is_valid(q)
        s = secure(q, args.user)
        print(f"  {name:12s} valid={ok}  shard={s.shard_key!r}")

    if not args.live:
        print("\nOffline complete. Re-run with --live to hit Qdrant.")
        return 0

    # Optional live path
    embedder = pyqql.HttpEmbedder(
        endpoint="http://localhost:11434/v1/embeddings",
        model="nomic-embed-text",
        dimension=768,
    )
    client = pyqql.Client("http://localhost:6333", embedder=embedder)
    print("\n── live execute ──")
    try:
        report = client.execute(secured)
        print(json.dumps(report, indent=2)[:500])
    except Exception as e:
        print(f"  execute failed (expected if collection missing): {e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
