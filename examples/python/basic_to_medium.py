#!/usr/bin/env python3
"""
Basic → Medium (Python / pyqql)

Offline walkthrough:
  1. parse / is_valid
  2. explain
  3. compile_query
  4. inject_filter (logical isolation)
  5. SHARD in QQL + Stmt.shard_key (physical routing)
  6. hybrid shorthand
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("QQL_LIB", str(_ROOT / "crates" / "pyqql")))

import pyqql  # noqa: E402

print(f"pyqql {getattr(pyqql, '__version__', '?')}\n")

q = "QUERY TEXT 'cardiology treatment protocols' FROM medical_records USING dense LIMIT 5"
print("1. is_valid:", pyqql.is_valid(q))
print("   parsed Stmt OK\n")

print("2. explain()")
print(pyqql.explain(q).get("plan", ""))
print()

route = pyqql.compile_query(q)
print("3. compile_query()")
print(f"   stmt_type={route.get('stmt_type')}  {route.get('method')} {route.get('path')}\n")

# Fast JSON-only parse — same AST as parse(), no Python Stmt objects.
print("3b. parse_json()")
print(f"   {pyqql.parse_json(q)[:120]}…\n")

secured = pyqql.inject_filter(q, "tenant_id", "=", "hospital-east")
print("4. inject_filter(tenant_id = hospital-east)")
print("   filter:", json.dumps(secured.to_dict().get("Query", {}).get("filter"), indent=2)[:300])
print()

# Preferred: write SHARD in QQL (security + routing in the language)
sharded_q = """
    QUERY TEXT 'cardiology treatment protocols'
    FROM medical_records
    USING dense
    SHARD 'hospital-east'
    LIMIT 5
"""
stmt = pyqql.parse(sharded_q)[0]
print("5a. SHARD in QQL → shard_key =", repr(stmt.shard_key))
# Host resolve-after-parse (optional): property only, no inject_shard_key
plain = pyqql.parse(q)[0]
plain.shard_key = "hospital-east"
print("5b. Stmt.shard_key = … →", repr(plain.shard_key))
print()

hybrid = """
    QUERY TEXT 'emergency neurological assessment'
    FROM medical_records
    USING HYBRID DENSE dense SPARSE sparse FUSION RRF
    WHERE priority = 'high'
    SHARD 'hospital-east'
    LIMIT 5
"""
print("6. hybrid + SHARD — is_valid:", pyqql.is_valid(hybrid))
print(pyqql.explain(hybrid).get("plan", ""))
print("\nDone.")
