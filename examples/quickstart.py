#!/usr/bin/env python3
"""QQL 5-line happy path (Python / pyqql) — offline, no Qdrant server.

Runs in CI: parse → hybrid CTE as text → tenant isolation → bind →
compile → `:rows` ingest shape. Live execution (`Client.execute`,
`Client.upsert_many`) needs a server and is covered by live integration
tests, not here.
"""

import pyqql

# 1. One language for complex retrieval — hybrid fusion as text.
q = """
WITH
  dense  AS (QUERY TEXT 'vector databases' FROM docs USING dense  LIMIT 100),
  sparse AS (QUERY TEXT 'vector databases' FROM docs USING sparse LIMIT 100)
QUERY FUSION RRF FROM docs PREFETCH (dense, sparse) LIMIT 10
"""
assert pyqql.is_valid(q)

# 2. Tenant isolation: inject_filter always; SHARD routes.
stmt = pyqql.parse(q)[0]
stmt.inject_filter("tenant_id", "=", "acme")
stmt.shard_key = "acme"

# 3. Bind + compile offline — the exact REST route, no I/O.
param_q = "QUERY TEXT :q FROM docs WHERE tenant_id = :t LIMIT :lim"
route = pyqql.compile_query(
    param_q, {"q": "vector databases", "t": "acme", "lim": 10}
)
assert route["method"] == "POST"
assert route["path"] == "/collections/docs/points/query"

# 4. Ingest shape is data, not text: point dicts splice into `:rows`.
# Live: client.upsert_many("docs", rows, batch_size=100).
rows = [
    {"id": 1, "vector": {"dense": [0.1, 0.2, 0.3]}, "tag": "a"},
    {"id": 2, "vector": {"dense": [0.4, 0.5, 0.6]}, "tag": "b"},
]
tpl = pyqql.parse("UPSERT INTO docs VALUES :rows")[0]
bound = tpl.bind({"rows": rows})
assert "id: 1" in str(bound) and "id: 2" in str(bound)
assert callable(pyqql.Client.upsert_many)

print("quickstart ok: hybrid + isolation + bind + :rows")
