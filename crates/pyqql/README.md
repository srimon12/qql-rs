# pyqql

Native Python bindings for QQL (parser, plan, execute) via PyO3.

## Proposition

Write Qdrant operations as QQL instead of hand-built JSON. Same language as the
CLI and other SDKs: hybrid search, multitenancy (`inject_filter` + `SHARD`),
schema-as-code, REST or gRPC.

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
| `Client(url, api_key=None, use_grpc=False, embedder=None)` | Execute against Qdrant |
| `HttpEmbedder(endpoint, model, dimension, api_key="")` | OpenAI-compatible embeddings |
| `parse` / `is_valid` / `tokenize` | Frontend |
| `inject_filter(query\|Stmt, field, op, value)` | Host isolation (AST) |
| `Stmt.shard_key` | Same field as QQL `SHARD '…'` (get/set; no `inject_shard_key`) |
| `compile_query` / `explain` | Offline plan / REST projection |
| `execute` / `execute_async` | One-shot free functions |

### `inject_filter` operators

Accepted: `=`, `>`, `>=`, `<`, `<=` (and aliases).  
Rejected: `!=`, `IN`, … — write those in QQL or inject equality only.

### Isolation vs routing

| Concern | API | Wire |
|---------|-----|------|
| Isolation | `inject_filter` / `WHERE` | REST/gRPC **Filter** |
| Routing | `SHARD '…'` or `stmt.shard_key` | REST `shard_key` / gRPC `ShardKeySelector` |
| Partition DDL | `CREATE SHARD KEY '…'` | Admin shard-key API |

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
