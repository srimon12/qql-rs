# Python SDK (`pyqql`) Reference & Examples

Native Python bindings via PyO3.

## Install

```bash
pip install pyqql
```

---

## 1. Multi-Tenant Filter Injection

Parse a user query, inject tenant isolation, execute -- single call site, guaranteed safe.

```python
from pyqql import parse, inject_filter, Client

client = Client("http://localhost:6333")

# User query from UI / API
stmt = parse("QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10")

# Platform injects tenant filter -- recurses into CTEs and prefetches
inject_filter(stmt, "tenant_id", "=", "honeywell")

# Unified execute accepts Stmt objects directly
result = client.execute(stmt)
```

---

## 2. Client Constructor

```python
Client(
    url="http://localhost:6333",
    api_key=None,
    use_grpc=False,
    embedder=None,
)
```

- `url`: Qdrant REST (or gRPC) endpoint
- `api_key`: Optional API key for authenticated Qdrant instances (sent as `api-key` header)
- `use_grpc`: Set `True` to use gRPC transport (requires `--features grpc` build)
- `embedder`: A `pyqql.HttpEmbedder` instance or a dict with `endpoint`, `api_key`, `model`, `dimension` keys

```python
# With API key
client = Client("https://qdrant.example.com:6333", api_key="sk-...")

# With gRPC
client = Client("http://localhost:6334", use_grpc=True)

# With embedder
client = Client("http://localhost:6333", embedder={
    "endpoint": "http://localhost:11434/v1/embeddings",
    "api_key": "",
    "model": "all-minilm:l6-v2",
    "dimension": 384,
})
```

---

## 3. Schema-as-Code + Multi-Statement

`execute()` auto-detects semicolons -- no separate `parse_all` needed for execution. Same-collection QUERY statements are automatically grouped into a single network call.

```python
from pyqql import Client

client = Client("http://localhost:6333")

# Single statement
client.execute("CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)")

# Multi-statement -- semicolons auto-detected, batch-executed
client.execute("""
  CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
    WITH HNSW (m = 16)
    WITH PARAMS (replication_factor = 3, shard_number = 4);

  CREATE INDEX ON COLLECTION docs FOR title TYPE text;
  CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
  CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
""")
```

For programmatic manipulation (inspect/modify before executing), use `parse_all`:

```python
from pyqql import parse_all, Client

client = Client("http://localhost:6333")
stmts = list(parse_all("Q1; Q2; Q3;"))

# Inspect, inject filters, set shard keys...
for stmt in stmts:
    stmt.shard_key = "acme"
    stmt.inject_filter("tenant_id", "=", "acme")

# Execute all at once (auto-batched)
results = client.execute(stmts)
```

---

## 4. Batch Execution

`execute()` accepts four input types. Lists and semicolon-delimited multi-statements are automatically batched -- same-collection QUERYs share a single network call.

```python
from pyqql import parse, Client

client = Client("http://localhost:6333")

# Single string
result = client.execute("QUERY 'search' FROM docs USING dense LIMIT 10")

# Single Stmt (pre-parsed, reusable)
stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10")
result = client.execute(stmt)

# Multi-statement (semicolons) -- simplest for scripts
results = client.execute(
    "QUERY 'a' FROM docs USING dense LIMIT 10;"
    "QUERY 'b' FROM docs USING dense LIMIT 10;"
    "QUERY 'c' FROM docs USING dense LIMIT 10;"
)
# -> 3 queries, 1 network call

# Batch from a list of strings
results = client.execute([
    "QUERY 'a' FROM docs USING dense LIMIT 10",
    "QUERY 'b' FROM docs USING dense LIMIT 10",
    "QUERY 'c' FROM docs USING dense LIMIT 10",
])

# Batch from pre-parsed Stmts (parse once, reuse)
stmts = [parse(f"QUERY '{q}' FROM docs USING dense LIMIT 10") for q in ("a", "b", "c")]
results = client.execute(stmts)
```

---

## 5. Stmt Manipulation

The `Stmt` object supports programmatic modification before execution.

```python
from pyqql import parse, inject_filter

stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10")

# Read / write the shard key
stmt.shard_key = "acme"
print(stmt.shard_key)  # -> "acme"

# Inject a tenant filter
stmt.inject_filter("tenant_id", "=", "acme")

# Serialise to JSON string or Python dict
print(stmt.to_json())
print(stmt.to_dict())
```

---

## 6. Complex Retrieval

Multi-stage hybrid retrieval with CTE, Fusion, and Rerank.

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

---

## 7. Async Execution

```python
import asyncio
from pyqql import Client

async def main():
    client = Client("http://localhost:6333")
    result = await client.execute_async("QUERY 'search' FROM docs USING dense LIMIT 5")

asyncio.run(main())
```

---

## 8. Free Functions

```python
stmt = parse("QUERY 'x' FROM docs LIMIT 5")           # Parse single statement
stmts = parse_all("Q1; Q2;")                           # Parse multi-statement script
stmts = parse_batch(["Q1", "Q2"])                      # Parse batch
ok = is_valid("QUERY 'x' FROM docs LIMIT 5")           # Validate without parsing
tokenized = tokenize("QUERY 'x' FROM docs LIMIT 5")    # Lex into tokens
result = inject_filter(stmt, "tenant_id", "=", "acme") # Inject filter (mutates or returns new)
route = compile_query("QUERY 'x' FROM docs LIMIT 5")   # Lower to REST route (no execute)
```
