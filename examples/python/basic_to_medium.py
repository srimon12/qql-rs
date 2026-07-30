#!/usr/bin/env python3
"""
Basic → Medium (Python / pyqql)

Offline-first walkthrough of the core QQL UX:
  1. parse / is_valid
  2. explain (human-readable plan)
  3. compile_query (REST route projection)
  4. inject_filter  — host-side tenant isolation
  5. inject_shard_key — host-side physical shard routing
  6. hybrid search shorthand (USING HYBRID)

No Qdrant server required for this script.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

# Prefer workspace pyqql (0.1.4+) over any older site-packages install
_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("QQL_LIB", str(_ROOT / "crates" / "pyqql")))

import pyqql  # noqa: E402

print(f"pyqql {getattr(pyqql, '__version__', '?')}\n")

# ── 1. Validate & parse ──────────────────────────────────────────────
q = "QUERY TEXT 'cardiology treatment protocols' FROM medical_records USING dense LIMIT 5"
print("1. is_valid:", pyqql.is_valid(q))
stmt = pyqql.parse(q)[0]
print("   parsed Stmt OK\n")

# ── 2. Explain (no network) ──────────────────────────────────────────
print("2. explain()")
print(pyqql.explain(q).get("plan", ""))
print()

# ── 3. Compile to Qdrant REST route ──────────────────────────────────
route = pyqql.compile_query(q)
print("3. compile_query() → REST projection")
print(f"   stmt_type={route.get('stmt_type')}  {route.get('method')} {route.get('path')}\n")

# ── 4. inject_filter — recursive tenant isolation ────────────────────
secured = pyqql.inject_filter(q, "tenant_id", "=", "hospital-east")
print("4. inject_filter(tenant_id = hospital-east)")
print("   filter:", json.dumps(secured.to_dict().get("Query", {}).get("filter"), indent=2)[:300])
print()

# ── 5. inject_shard_key — physical shard routing ─────────────────────
pyqql.inject_shard_key(secured, "hospital-east")
print("5. inject_shard_key('hospital-east')")
print(f"   shard_key={secured.shard_key!r}\n")

# ── 6. Hybrid shorthand (QQL 1.2) ────────────────────────────────────
hybrid = """
    QUERY TEXT 'emergency neurological assessment'
    FROM medical_records
    USING HYBRID DENSE dense SPARSE sparse FUSION RRF
    WHERE priority = 'high'
    LIMIT 5
"""
print("6. hybrid shorthand — is_valid:", pyqql.is_valid(hybrid))
print(pyqql.explain(hybrid).get("plan", ""))
print()

# ── Optional live client (skipped unless Qdrant is up) ───────────────
try:
    client = pyqql.Client("http://localhost:6333", use_grpc=False)
    plan = client.explain("QUERY TEXT 'demo' FROM docs USING dense LIMIT 1")
    plan_text = plan.get("plan", "") if isinstance(plan, dict) else str(plan)
    print("7. Client.explain (live):", plan_text.split("\n")[0] if plan_text else "ok")
except Exception as e:
    print(f"7. Client (optional): skipped — {type(e).__name__}")

print("\nDone. Next: medium_to_expert.py")
