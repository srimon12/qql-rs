# pyqql

Native Python bindings for the Qdrant Query Language (QQL) parser, router, and execution engine, compiled with PyO3.

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **Automated Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI) for text-to-vector search
- **Native Route Lowering**: Lower QQL queries to typed `{ method, path, payload }` route dicts via `compile_query`
- **Native parsing**: Rust-speed QQL parsing in Python returning typed `Stmt` objects or Python dicts
- **Filter injection**: Add tenant isolation filters programmatically
- **Smart batching**: Auto-batches contiguous same-collection query/mutation statements into single network calls
- **Shard key**: Read/write the shard key on QUERY, COUNT, SCROLL, UPSERT, and DELETE statements
- **Validation**: Check if a query string is valid QQL

## Compatibility

- **Python 3.8+**: Published wheels use Python's stable ABI (`abi3-py38`) and support Python 3.8 and newer.
- **REST and gRPC**: Published wheels include both transports by default.

## Installation

```bash
pip install pyqql
```

## Quick Start

```python
import asyncio
import pyqql

# 1. Connect to live Qdrant with optional custom embedding provider (e.g. Ollama)
embedder = pyqql.HttpEmbedder(
    endpoint="http://localhost:11434/v1/embeddings",
    model="all-minilm:l6-v2",
    dimension=384,
    api_key=""
)

client = pyqql.Client(
    url="http://localhost:6333",
    api_key="optional-qdrant-secret",
    use_grpc=False,
    embedder=embedder
)

# Execute QQL query (auto-embeds text to vector)
result = client.execute("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
print(result)

# Explain query execution plan
plan = client.explain("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
print(plan)

# Async execution example
async def main():
    report = await client.execute_async("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
    print(report)

asyncio.run(main())

# 2. Pure AST Parsing & Filter Injection
stmt = pyqql.parse("QUERY 'vector database' FROM docs USING dense LIMIT 10")[0]
valid = pyqql.is_valid("QUERY 'test' FROM docs")
secured_stmt = pyqql.inject_filter("QUERY 'patients' FROM medical LIMIT 5", "org_id", "=", "acme-corp")

# 3. Working with Stmt objects
ast_dict = stmt.to_dict()                    # Python dict
ast_json = stmt.to_json()                    # JSON string
stmt.shard_key = "shard-01"                  # setter (QUERY/COUNT/SCROLL/UPSERT/DELETE only)
stmt.inject_filter("tenant_id", "=", "acme") # mutate in-place

# 4. Free-function execute (convenience)
result = pyqql.execute("SHOW COLLECTIONS", url="http://localhost:6333")

# 5. Lower to Qdrant route without executing
route = pyqql.compile_query("QUERY 'search' FROM docs LIMIT 10")
# route = { "method": "POST", "path": "/collections/docs/points/query", "payload": {...} }
```

## Execution Results & Error Handling

### ExecutionReport Format

All execution methods return an `ExecutionReport` dictionary:

```python
{
    "ok": True,
    "results": [
        {
            "ok": True,
            "operation": "QUERY",
            "message": "Found 5 hits",
            "data": [...]
        }
    ],
    "succeeded": 1,
    "failed": 0
}
```

### Failure Policy (`on_error`)

| Policy | Behavior |
|---|---|
| `"stop"` (default) | Halts execution on the first error and raises a Python exception. |
| `"continue"` | Continues executing remaining statements, collecting failures into `results` with `ok: False`. |

### Exceptions

`pyqql` raises standard Python exception types:
- `SyntaxError` — QQL parse or lex errors.
- `TypeError` — Invalid option or argument types.
- `ValueError` — Invalid configuration values or unaccepted filter operators.
- `RuntimeError` — Network transport or Qdrant backend failures.

## Filter Injection Operators

`inject_filter` accepts comparison operators:
- **Accepted**: `=`, `==`, `eq`, `>`, `gt`, `>=`, `gte`, `<`, `lt`, `<=`, `lte`
- **Rejected**: `!=`, `neq`, `<>`, `in`, `is_null` (raises `SyntaxError` — wrap with `NOT` or write in QQL query)

## API Summary

| Export | Description |
|---|---|
| `Client(url, api_key, use_grpc, embedder)` | Client for executing QQL against a live Qdrant database |
| `HttpEmbedder(endpoint, model, dimension, api_key)` | First-class HTTP embedding provider configuration |
| `Stmt` | Parsed statement object with `inject_filter()`, `to_json()`, `to_dict()`, `shard_key` property |
| `parse(input)` | Parse one statement or a semicolon-delimited script into a list of `Stmt` objects |
| `is_valid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Inject tenant filter into statement AST (accepts str or Stmt) |
| `tokenize(input)` | Tokenize QQL string for syntax highlighting or inspection |
| `compile_query(input)` | Lower QQL statement into typed `{ method, path, payload }` route dict |
| `explain(query)` | Inspect the execution plan without executing network calls (accepts str or Stmt) |
| `execute(query, ..., on_error="stop")` | Free-function convenience execute |
| `execute_async(query, ..., on_error="stop")` | Free-function async execute |
| `Client.execute(query, on_error="stop")` | Execute a string, Stmt, list[str], or list[Stmt] |
| `Client.execute_async(query, on_error="stop")` | Async variant of execute |
| `Client.explain(query)` | Inspect execution plan (accepts str or Stmt) |
| `__version__` | Package runtime version string |

## Documentation Links

- [QQL Syntax Guide](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md)
- [Filter Documentation](https://github.com/srimon12/qql-rs/blob/main/docs/filters.md)
- [Filter Injection Guide](https://github.com/srimon12/qql-rs/blob/main/docs/inject_filter.md)
- [Changelog](https://github.com/srimon12/qql-rs/blob/main/CHANGELOG.md)
