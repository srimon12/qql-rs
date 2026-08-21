# Python SDK (`pyqql`) Reference & Examples

Native Python bindings via PyO3.

Language surface includes **Qdrant 1.19 / QQL 1.4** features expressible in QQL
(`SHOW QUOTAS`, memory/`turbo4`, `MATCH PREFIX`, `SLICE`, `PARAMS (idf = …)`).
Those statements execute through the same `Client.execute` path as older syntax
when the connected backend supports them (quotas: **REST only**).

**Route affinity** (`X-Qdrant-Route-Affinity`) is exposed on `Client` via the
`route_affinity` constructor keyword (readable with `client.route_affinity`).
See §2b below.

## Install

```bash
pip install pyqql
```

---

## 1. Multi-tenant isolation + optional routing

**Isolation** = `inject_filter` (always on untrusted QQL).  
**Routing** = `SHARD '…'` in QQL (preferred) or `stmt.shard_key` after parse.  
There is **no** `inject_shard_key`.

```python
from pyqql import parse, inject_filter, Client

client = Client("http://localhost:6333")

# Preferred when tenant is known: SHARD in the language
stmt = parse("""
  QUERY TEXT 'supply chain risks' FROM sec10k USING dense
  SHARD 'honeywell' LIMIT 10
""")[0]

# Always force isolation (recursive into CTEs / prefetches)
inject_filter(stmt, "tenant_id", "=", "honeywell")

result = client.execute(stmt)

# Host-resolved routing after parse (same AST field as SHARD):
# stmt.shard_key = "honeywell"
```

---

## 2. Client Constructor

```python
Client(
    url="http://localhost:6333",
    api_key=None,
    use_grpc=False,
    embedder=None,
    route_affinity=None,
)
```

- `url`: Qdrant REST (or gRPC) endpoint
- `api_key`: Optional API key for authenticated Qdrant instances (sent as `api-key` header)
- `use_grpc`: Set `True` to use gRPC transport (requires `--features grpc` build)
- `embedder`: A `pyqql.HttpEmbedder` instance or a dict with `endpoint`, `api_key`, `model`, `dimension` keys
- `route_affinity`: Optional Qdrant 1.19 read-affinity key, pinning reads to a
  stable replica. Sent as `X-Qdrant-Route-Affinity` (REST) / gRPC metadata
  `x-qdrant-route-affinity`. Empty string is treated as unset. Readable via
  `client.route_affinity`.

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

# With read affinity (Qdrant 1.19+)
client = Client("http://localhost:6333", route_affinity="session-acme-42")
print(client.route_affinity)  # "session-acme-42"
```

---

## 3. Schema-as-Code + Multi-Statement

`execute()` and `parse()` auto-detect semicolon-delimited scripts. Same-collection QUERY statements are automatically grouped into a single network call.

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

For programmatic manipulation (inspect/modify before executing), use `parse()`:

```python
from pyqql import parse, Client

client = Client("http://localhost:6333")
stmts = parse("QUERY 'a' FROM docs LIMIT 5; QUERY 'b' FROM docs LIMIT 5")

# Inspect, inject filters, set shard keys...
for stmt in stmts:
    stmt.shard_key = "acme"
    stmt.inject_filter("tenant_id", "=", "acme")

# Execute all at once (auto-batched)
results = client.execute(stmts)
```

For raw AST JSON without Python object allocation (parity with Node's
`parseJson`), use `parse_json()`:

```python
from pyqql import parse_json

ast_json = parse_json("QUERY 'a' FROM docs LIMIT 5")  # JSON string of the AST array
```

---

## 4. Batch Execution

`execute()` accepts four input types. Lists and semicolon-delimited scripts are
automatically batched. Pass `on_error="continue"` to collect per-statement
failures; the default is `"stop"`.
Every input form returns an `ExecutionReport` dict with `ok`, ordered
`results`, `succeeded`, and `failed` fields.

```python
from pyqql import parse, Client

client = Client("http://localhost:6333")

# Single string
result = client.execute("QUERY 'search' FROM docs USING dense LIMIT 10")

# Single Stmt (pre-parsed, reusable)
stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10")[0]
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
stmts = [parse(f"QUERY '{q}' FROM docs USING dense LIMIT 10")[0] for q in ("a", "b", "c")]
results = client.execute(stmts)
```

---

## 5. Stmt Manipulation

The `Stmt` object supports programmatic modification before execution.

```python
from pyqql import parse, inject_filter

stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10")[0]

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

Vector roles: `USING dense` / `USING sparse` / `USING colbert` without `AS`
are resolved from the **collection schema** before embedding (dense, sparse, or
multivector). Offline or explicit roles use `AS DENSE`, `AS SPARSE`, or
`AS MULTI`. Names are not special-cased by spelling.

Hybrid shorthand (dense + sparse fusion, same expand as `QUERY HYBRID`):

```python
client.execute(
    "QUERY TEXT 'vector databases' FROM docs "
    "USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT 10"
)
# or: "QUERY 'vector databases' FROM docs USING HYBRID LIMIT 10"
```

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
QUERY RERANK TEXT 'vector databases' MODEL 'answerai-colbert-small-v1'
  FROM docs
  USING colbert
  PREFETCH (fused)
  LIMIT 10
"""

result = client.execute(query)

# Multivector nearest (collection has colbert WITH MULTIVECTOR)
# client.execute("QUERY TEXT 'q' FROM docs USING colbert LIMIT 10")
# Offline without schema: "... USING colbert AS MULTI LIMIT 10"
#
# Cross-encoder (client-side pair scorer; host needs rerank_pairs):
# WITH c AS (QUERY TEXT 'q' FROM docs USING dense LIMIT 50)
# QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD text
#   FROM docs PREFETCH (c) LIMIT 10
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
stmt = parse("QUERY 'x' FROM docs LIMIT 5")[0]        # Parse one statement
stmts = parse("QUERY 'a' FROM docs; COUNT FROM docs")        # Parse a script
ok = is_valid("QUERY 'x' FROM docs LIMIT 5")           # Validate without returning the AST
tokenized = tokenize("QUERY 'x' FROM docs LIMIT 5")    # Lex into tokens
result = inject_filter(stmt, "tenant_id", "=", "acme") # Inject filter (mutates or returns new)
route = compile_query("QUERY 'x' FROM docs LIMIT 5")   # Lower to REST route (no execute)
```
