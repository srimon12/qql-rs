# Python SDK (`pyqql`) Reference & Examples

Native Python bindings via PyO3.

Language surface includes **Qdrant 1.19 / QQL 1.5** features expressible in QQL
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

Sparse IDF is **not** `inject_filter` and not a JSON corpus object. Write it in
QQL. `compile_query` / execute lower `WHERE tenant_id = '…'` to Qdrant’s
`params.idf.corpus` filter JSON — hosts do not build that dict.

```python
from pyqql import bind, compile_query

# Isolation + routing + tenant-local BM25 stats (three different layers)
qql = """
QUERY TEXT :q FROM sec10k USING sparse
WHERE tenant_id = :tenant
SHARD :tenant
PARAMS (idf = WHERE tenant_id = :tenant)
LIMIT 10
"""
bound = bind(qql, {"q": "supply chain", "tenant": "honeywell"})
route = compile_query(bound)
# route["payload"]["params"]["idf"] ==
#   {"corpus": {"must": [{"key": "tenant_id", "match": {"value": "honeywell"}}]}}
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

Typed exceptions: every error is a `pyqql.QqlError` subclass carrying
`.code` / `.kind` / `.span` — catch `QqlSyntaxError`, `QqlValidationError`,
`QqlExecutionError`, `QqlTransportError`, or `QqlBackendError` by code
instead of string-matching (each also subclasses the builtin category it
used to raise, so `except SyntaxError` / `ValueError` / `RuntimeError`
keep working). Re-binding an already-bound `Stmt` with new params raises
`QQL-BIND-ALREADY-BOUND`; an empty script raises
`QQL-VALIDATION-EMPTY-SCRIPT`; a `close()`d client raises
`QQL-CLIENT-CLOSED` on the next execute.

Vector parameters: prefer the implicit `QUERY :vec USING <model> FROM …`
spelling. `QUERY VECTOR :vec` now parses to the same statement (since
0.3.2), but implicit+USING is the canonical documented form. Matrix params
(list of number lists) bind as ColBERT multi-vectors on the `Stmt` path,
and array-likes with `tolist()` (numpy arrays) bind directly.
`LIMIT 0` is rejected at parse time: Qdrant's query API requires
`limit >= 1` (verified live — the server answers 422), so the failure
surfaces at the parse gate instead of as a runtime 422. Unbound
placeholders fail the same way on every path — `execute(str)` without
params raises `QQL-BIND-MISSING-PARAM` before any request leaves.

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

## 7. Parameter Binding & Prepared Queries

Substitute named (`:name`) or positional (`?`) parameters safely into queries:

```python
from pyqql import Client, bind, parse

client = Client("http://localhost:6333")

# Direct execution with named parameters
report = client.execute(
    "QUERY TEXT :query FROM docs WHERE category = :cat AND rating >= :min_rating LIMIT :limit",
    params={"query": "machine learning", "cat": "tech", "min_rating": 4.5, "limit": 10},
)

# Direct execution with positional parameters
report = client.execute(
    "QUERY TEXT ? FROM docs WHERE category = ? LIMIT ?",
    params=["machine learning", "tech", 10],
)

# Prepared statements: parse once, execute repeatedly with different parameters
stmt = parse("QUERY TEXT :query FROM docs WHERE category = :cat LIMIT :limit")[0]
report = client.execute(stmt, params={"query": "neural nets", "cat": "ai", "limit": 5})

# Direct compile route from prepared statement (no re-parsing required)
route = stmt.compile_route(params={"query": "neural nets", "cat": "ai", "limit": 5})
print(route["path"], route["payload"])

# Standalone AST statement binding
bound_stmt = stmt.bind({"query": "search term", "cat": "tech", "limit": 5})
print(bound_stmt)  # Formatted QQL string

# Nested dictionary parameters (automatically flattened to dotted keys like :loc.lat)
geo_query = "QUERY 'coffee' FROM venues WHERE location GEO_RADIUS { center: {lat: :loc.lat, lon: :loc.lon}, radius: :rad } LIMIT 5"
report = client.execute(geo_query, params={"loc": {"lat": 52.52, "lon": 13.40}, "rad": 1000})

# Statement-scoped parameters for multi-statement batches
# (length must match the statement count exactly — QQL-BIND-BATCH-LENGTH
# otherwise; a scalar list like [1, 2] is a shared positional list, never
# per-statement)
batch_stmts = [
    "QUERY TEXT :q FROM docs LIMIT 5",
    "QUERY TEXT :q FROM articles LIMIT 10",
]
report = client.execute(batch_stmts, params=[{"q": "quantum"}, {"q": "relativity"}])

# Vector truncation for readable logging (avoids dumping hundreds of float literals)
vec_query = "QUERY :vec FROM docs LIMIT 5"
print(bind(vec_query, {"vec": [0.1] * 384}, truncate_vectors=True))
# -> QUERY [0.10, 0.10, ... (384 dims)] FROM docs LIMIT 5
```

---

## 8. Typed Result Accessors & `ExecutionReport`

`client.execute()` returns an `ExecutionReport` (subclass of `dict` that retains full backward compatibility with `report["results"]` and `report["ok"]`).

```python
from pyqql import Client, ScoredPoint

client = Client("http://localhost:6333")

# 1. Accessing hits with .hits() -> List[ScoredPoint]
report = client.execute("QUERY TEXT 'neural search' FROM docs LIMIT 5")
for hit in report.hits():
    # hit is a ScoredPoint dataclass:
    print(hit.id)         # int (e.g. 42) or str (UUID) -- numeric IDs are preserved as ints!
    print(hit.score)      # float, e.g. 0.892
    print(hit.payload)    # dict with document payload
    print(hit.text)       # shortcut for hit.payload.get("text")
    print(hit["title"])   # dict-like subscript access to hit.payload
    print(hit.get("url")) # dict-like get() with optional default

# 2. Shortcut: execute_hits() returns List[ScoredPoint] directly
hits = client.execute_hits("QUERY TEXT 'neural search' FROM docs LIMIT 5")

# 3. Facet queries -> .facet() returns normalized [{value: ..., count: ...}]
report = client.execute("FACET category FROM docs LIMIT 10")
for item in report.facet():
    print(item["value"], item["count"])

# 4. Count queries -> .count() returns integer count
report = client.execute("COUNT FROM docs WHERE category = 'tech'")
print("Total matches:", report.count())

# 5. Point retrieval -> .points() returns retrieved points
report = client.execute("QUERY POINTS (1, 2, 3) FROM docs")
points = report.points()
```

---

## 9. Async Execution

```python
import asyncio
from pyqql import Client

async def main():
    client = Client("http://localhost:6333")
    result = await client.execute_async(
        "QUERY TEXT :q FROM docs USING dense LIMIT :lim",
        params={"q": "search", "lim": 5},
    )

asyncio.run(main())
```

---

## 10. Free Functions & Explain

```python
import pyqql

stmt = pyqql.parse("QUERY 'x' FROM docs LIMIT 5")[0]        # Parse one statement
stmts = pyqql.parse("QUERY 'a' FROM docs; COUNT FROM docs") # Parse a script
ok = pyqql.is_valid("QUERY 'x' FROM docs LIMIT 5")          # Validate without returning the AST
tokenized = pyqql.tokenize("QUERY 'x' FROM docs LIMIT 5")   # Lex into tokens
result = pyqql.inject_filter(stmt, "tenant_id", "=", "acme") # Inject filter
route = pyqql.compile_query("QUERY 'x' FROM docs LIMIT 5")  # Lower to REST route (no execute)

# Hierarchical ASCII tree plan
plan_dict = pyqql.explain("QUERY TEXT 'hello' FROM docs USING dense LIMIT 10")
print(plan_dict["plan"])
# Query Plan
# └── Target: docs
#     ├── Query: text('hello') via dense
#     └── Limit: 10

# Standalone parameter binding
bound = pyqql.bind("QUERY TEXT :q FROM docs LIMIT :lim", {"q": "test", "lim": 10})
```
