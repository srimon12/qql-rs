# Python SDK (`pyqql`) Reference & Examples

Native Python bindings via PyO3.

## Install

```bash
pip install pyqql
```

---

## 1. Multi-Tenant Filter Injection

Parse a user query, inject tenant isolation, execute — single call site, guaranteed safe.

```python
from pyqql import parse, inject_filter, Client

client = Client("http://localhost:6333")

# User query from UI / API
stmt = parse("QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10")

# Platform injects tenant filter — recurses into CTEs and prefetches
inject_filter(stmt, "tenant_id", "=", "honeywell")

result = client.execute_stmt(stmt)
```

---

## 2. Schema-as-Code

The same `.qql` file works from Python, Rust, Node, and WASM. Parse, inspect, execute.

```python
from pyqql import parse_all, Client

schema = """
CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
  WITH HNSW (m = 16)
  WITH PARAMS (replication_factor = 3, shard_number = 4);

CREATE INDEX ON COLLECTION docs FOR title TYPE text;
CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
"""

client = Client("http://localhost:6333")
for stmt in parse_all(schema):
    client.execute_stmt(stmt)
```

---

## 3. Complex Retrieval

Multi-stage hybrid retrieval with CTE, Fusion, and Rerank — one string.

```python
from pyqql import Client

client = Client("http://localhost:6333")

query = """
WITH
  dense  AS (QUERY TEXT 'vector databases' USING dense  LIMIT 100),
  sparse AS (QUERY TEXT 'vector databases' USING sparse LIMIT 100),
  fused  AS (
    QUERY FUSION RRF FROM docs
      PREFETCH (dense WHERE priority = 'high', sparse)
      LIMIT 50
  )
QUERY RERANK TEXT 'vector databases' MODEL 'bge-reranker'
  FROM docs
  USING colbert
  PREFETCH (fused)
  LIMIT 10
"""

result = client.execute(query)
```
