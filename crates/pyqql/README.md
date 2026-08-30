# pyqql

Native Python bindings for QQL (parser, plan, execute) via PyO3.

## Proposition

Write Qdrant operations as QQL instead of hand-built JSON. Same language as the
CLI and other SDKs: hybrid search, multitenancy (`inject_filter` + `SHARD`),
schema-as-code, REST or gRPC. Language surface tracks **Qdrant ≥ 1.19**
(quotas, `memory` placement, `MATCH PREFIX` / `SLICE`, sparse `idf`, `turbo4`).

## Install

```bash
pip install pyqql
```

Python **3.8+** (stable ABI wheels). REST + gRPC included.

## Quick start

```python
import pyqql

embedder = pyqql.HttpEmbedder(
    endpoint="http://localhost:11434/v1/embeddings",
    model="all-minilm:l6-v2",
    dimension=384,
)
client = pyqql.Client("http://localhost:6333", embedder=embedder)

report = client.execute(
    "QUERY TEXT 'cardiology' FROM medical_records USING dense LIMIT 5"
)
print(report)  # ExecutionReport: ok, results[], succeeded, failed

# Isolation (always on untrusted QQL)
stmt = pyqql.parse("QUERY TEXT 'risks' FROM sec10k USING dense LIMIT 10")[0]
pyqql.inject_filter(stmt, "tenant_id", "=", "honeywell")

# Routing (custom sharding): prefer SHARD in QQL, or set after parse
#   QUERY ... SHARD 'honeywell' LIMIT 10
stmt.shard_key = "honeywell"
client.execute(stmt)
```

## API summary

| Export | Role |
|--------|------|
| `Client(url, api_key=None, use_grpc=False, embedder=None, route_affinity=None)` | Execute against Qdrant |
| `HttpEmbedder(endpoint, model, dimension, api_key="")` | OpenAI-compatible embeddings |
| `parse` / `parse_json` / `is_valid` / `tokenize` | Frontend |
| `inject_filter(query\|Stmt, field, op, value)` | Host isolation (AST) |
| `Stmt.shard_key` | Same field as QQL `SHARD '…'` (get/set; no `inject_shard_key`) |
| `compile_query` / `explain` | Offline plan / REST projection |
| `bind(query, params)` | Substitute `:name` (dict) or `?` (list) |
| `execute` / `execute_async` | One-shot free functions (`params=` same as `bind`) |

### `inject_filter` operators

Accepted: `=`, `>`, `>=`, `<`, `<=` (and aliases).  
Rejected: `!=`, `IN`, … — write those in QQL or inject equality only.

### Isolation vs routing

| Concern | API | Wire |
|---------|-----|------|
| Isolation | `inject_filter` / `WHERE` | REST/gRPC **Filter** |
| Routing | `SHARD '…'` or `stmt.shard_key` | REST `shard_key` / gRPC `ShardKeySelector` |
| Partition DDL | `CREATE SHARD KEY '…'` | Admin shard-key API |

### Qdrant 1.19 notes

```python
# Quotas: REST only (default Client URL :6333). use_grpc=True → QQL-GRPC-QUOTA
client.execute("SHOW QUOTAS")
client.execute(
    "SET QUOTA (enabled = true, max_resident_memory_percent = 80, "
    "max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true"
)

# Filters / DDL that require Qdrant ≥ 1.19
client.execute(
    "CREATE INDEX ON COLLECTION docs FOR title TYPE keyword "
    "WITH (prefix = true, memory = 'cached')"
)
client.execute(
    "QUERY TEXT 'q' FROM docs USING dense WHERE title MATCH PREFIX 'Comp' LIMIT 5"
)
client.execute(
    "QUERY TEXT 'q' FROM docs USING sparse PARAMS (idf = 'global') LIMIT 5"
)
client.execute(
    "QUERY TEXT 'q' FROM docs USING sparse "
    "WHERE tenant_id = 'acme' SHARD 'acme' "
    "PARAMS (idf = WHERE tenant_id = 'acme') LIMIT 5"
)
```

`SET QUOTA` is a **full replace** of the cluster config.

### Route affinity (Qdrant 1.19+)

Pin reads to a stable replica with `route_affinity` at construction — sent as
the `X-Qdrant-Route-Affinity` header (REST) / `x-qdrant-route-affinity` metadata
(gRPC). Empty string is treated as unset. Readable via `client.route_affinity`.

```python
client = pyqql.Client("http://localhost:6333", route_affinity="session-acme-42")
print(client.route_affinity)  # "session-acme-42"
# One-shot convenience:
pyqql.execute("SHOW COLLECTIONS", url="http://localhost:6333", route_affinity="session-acme-42")
```

## Execution report

```python
{
  "ok": True,
  "results": [{"ok": True, "operation": "QUERY", "message": "…", "data": …}],
  "succeeded": 1,
  "failed": 0,
}
```

`on_error="stop"` (default) or `"continue"`.

## Docs

- [Syntax](../../docs/syntax.md) · [Filters](../../docs/filters.md) · [inject_filter](../../docs/inject_filter.md)
- [Multitenancy](../../skills/qql-skill/references/qql-multitenancy.md) · [Python skill](../../skills/qql-skill/references/python-sdk.md)
