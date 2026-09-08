#!/usr/bin/env python3
"""Smoke test: validate every QQL statement form the benchmark harness uses,
against the live Qdrant, on a throwaway collection. Run before bench.py."""
import json, sys
from pathlib import Path
import pyqql

DATA = Path(__file__).resolve().parent.parent / "data"
q = json.loads((DATA / "queries.json").read_text())
qvec = q["berlin"][0]["dense"][:384]                      # dense query vector
svec = q["berlin"][0]["sparse"]                            # sparse query vector
withp = [([0.1] * 128)] * 8                                # mini multivector

C = "vsq_smoke"
cl = pyqql.Client("http://localhost:6333")
parse = pyqql.parse
try:
    cl.execute(f"DROP COLLECTION {C}")
except pyqql.QqlError:
    pass

# 1. create collection: dense + sparse + colbert multivector
print(cl.execute(f"""
CREATE COLLECTION {C} (
  dense VECTOR(384, COSINE),
  bm25 SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
)"""))

# 2. upsert with named vectors (dense + sparse + colbert) and payload
up = f"""UPSERT INTO {C} VALUES {{
  id: 1, name: 'Cozy apartment in Mitte', district: 'Mitte', price: 120.0,
  vector: {{ dense: {json.dumps(qvec)},
             bm25: {{ indices: {json.dumps(svec['indices'])}, values: {json.dumps(svec['values'])} }},
             colbert: {json.dumps(withp)} }}
}}"""
print("upsert:", cl.execute(up))

# 3. dense query, vector literal
r = cl.execute(f"QUERY {json.dumps(qvec)} FROM {C} USING dense LIMIT 5")
print("dense query:", r[0] if isinstance(r, list) else r)

# 4. sparse query with param
stmt = parse(f"QUERY :sv FROM {C} USING bm25 LIMIT 5")[0]
r = cl.execute(stmt, params={"sv": svec})
print("sparse query:", r)

# 5. hybrid: CTEs + FUSION RRF (params)
hy = f"""WITH d AS (QUERY :dv FROM {C} USING dense LIMIT 10),
     s AS (QUERY :sv FROM {C} USING bm25 LIMIT 10)
QUERY FUSION RRF FROM {C} PREFETCH (d, s) LIMIT 10"""
stmt = parse(hy)[0]
r = cl.execute(stmt, params={"dv": qvec, "sv": svec})
print("hybrid rrf:", r)

# 6. colbert multivector query (literal nested arrays)
r = cl.execute(f"QUERY NEAREST VECTOR {json.dumps(withp)} FROM {C} USING colbert LIMIT 5")
print("colbert query:", r)

# 7. scroll, count, facet
print("scroll:", cl.execute(f"SCROLL FROM {C} LIMIT 10") if True else None)
print("count:", cl.execute(f"COUNT FROM {C} WHERE price < 200"))
cl.execute(f"CREATE INDEX ON COLLECTION {C} FOR district TYPE keyword")
import time as _t
for _ in range(10):
    try:
        print("facet:", cl.execute(f"FACET district FROM {C} LIMIT 10")); break
    except pyqql.QqlError:
        _t.sleep(0.3)

# 8. update payload + delete by filter
print("update:", cl.execute(f"UPDATE {C} SET PAYLOAD = {{price: 99.0}} WHERE district = 'Mitte'"))
print("delete:", cl.execute(f"DELETE FROM {C} WHERE district = 'Mitte'"))
print("count after:", cl.execute(f"COUNT FROM {C}"))

cl.execute(f"DROP COLLECTION {C}")
print("SMOKE OK")

# ---- round 2: param-bound upsert + param colbert query ----
C2 = "vsq_smoke2"
try:
    cl.execute(f"DROP COLLECTION {C2}")
except pyqql.QqlError:
    pass
cl.execute(f"""
CREATE COLLECTION {C2} (
  dense VECTOR(384, COSINE),
  bm25 SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
)""")
dense_q = q["legal"][0]["dense"]
colb = q["legal"][0]["colbert"]
stmt = parse(f"""UPSERT INTO {C2} VALUES {{
  id: 7, title: 'case', court: 'KG Berlin', year: 2011,
  vector: {{ dense: :d, bm25: :s, colbert: :m }}
}}""")[0]
r = cl.execute(stmt, params={"d": dense_q, "s": svec, "m": colb})
print("param upsert:", r)
stmt = parse(f"QUERY NEAREST VECTOR :mv FROM {C2} USING colbert LIMIT 3")[0]
print("param colbert query:", cl.execute(stmt, params={"mv": colb})["results"][0]["data"])
cl.execute(f"DROP COLLECTION {C2}")
print("SMOKE2 OK")
