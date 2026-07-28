# pyqql-edge

Local QQL execution for Python — qdrant-edge + fastembed-rs, zero network.

## Features

- **In-Process Vector Storage**: Run Qdrant search engine locally inside Python process with zero server daemon requirement
- **Embedded ONNX Inference**: Automatically fetch and run FastEmbed ONNX models on-device
- **Native Route Lowering**: Lower QQL queries to typed `{ method, path, payload }` route dicts via `compile_query`
- **Native Parsing**: Rust-speed QQL parsing in Python returning `Stmt` objects or Python dicts
- **Filter Injection**: Programmatically add tenant isolation filters
- **Validation**: Check if a query string is valid QQL
- **Smart Batching**: Auto-batches contiguous same-collection query/mutation statements

## Compatibility & Platforms

- **Python 3.8+**: Published wheels use Python's stable ABI (`abi3-py38`).
- **Supported Platforms**:
  - Linux x64 (`glibc`)
  - macOS arm64 (`Apple Silicon`)
  - Windows x64 (`msvc`)
- *Note: Prebuilt wheels are not published for macOS Intel (Darwin x64) because ONNX Runtime lacks Darwin x64 prebuilds.*

## Installation

```bash
pip install pyqql-edge
```

## Quick Start

```python
import pyqql_edge

# 1. Discover local ONNX models
models = pyqql_edge.list_embedding_models()
# [{'name': 'BGESmallENV15', 'model_code': 'Xenova/bge-small-en-v1.5', 'dim': 384, ...}, ...]

# 2. Edge execution — pick model (default BGESmallENV15 / 384-d)
client = pyqql_edge.local_executor(
    "./qdrant_data",
    on_disk_payload=False,
    model="bge-small-en-v1.5",        # enum name, HF code, or short alias
    cache_dir="/var/cache/fastembed", # optional
)

# Schema-aware text auto-embed (dense-only, sparse-only, and hybrid)
client.execute("CREATE COLLECTION docs HYBRID")
client.execute(
    'UPSERT INTO docs VALUES {id: "550e8400-e29b-41d4-a716-446655440001", text: "hello"}'
)
result = client.execute("QUERY 'hello' FROM docs USING dense LIMIT 10", on_error="stop")
print(result)

# 3. Parser & Filter Injection
stmt = pyqql_edge.parse("QUERY 'hello' FROM docs LIMIT 10")[0]
tokens = pyqql_edge.tokenize("QUERY 'test' FROM docs")
secured_stmt = pyqql_edge.inject_filter("QUERY 'search' FROM docs", "org_id", "=", "acme")
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

## Filter Injection Operators

`inject_filter` accepts comparison operators:
- **Accepted**: `=`, `==`, `eq`, `>`, `gt`, `>=`, `gte`, `<`, `lt`, `<=`, `lte`
- **Rejected**: `!=`, `neq`, `<>`, `in`, `is_null` (raises `SyntaxError` — wrap with `NOT` or write in QQL query)

## Edge Gotchas

| Gotcha | Reality |
|--------|---------|
| Point IDs | Integers or UUIDs only — `"doc-1"` is rejected |
| Text UPSERT into an existing collection | Auto-embedding follows the schema: dense-only gets dense, sparse-only gets sparse, hybrid gets both |
| `QUERY 'text'` on HYBRID | Dense+sparse topology is ambiguous, so specify the target with `USING <vector_name>` |
| `GROUP BY` / shard keys | Rejected clearly; never silently ignored in edge mode |
| Model locked at `local_executor()` | `USING MODEL 'other'` mismatches fail |
| Client lifetime | Call `close()` before deleting `data_dir` |

## API Summary

| Export | Description |
|---|---|
| `local_executor(data_dir, ...)` | Create a fully local edge Client backed by fastembed-rs & qdrant-edge |
| `list_embedding_models()` | List dense ONNX models available for `local_executor(model=...)` |
| `http_executor(data_dir, url, ...)` | Create an edge Client with local vector storage and remote HTTP embedder |
| `Stmt` | Parsed statement object with `inject_filter()`, `to_json()`, `to_dict()`, `shard_key` property |
| `parse(input)` | Parse one statement or a semicolon-delimited script into a list of `Stmt` objects |
| `is_valid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Inject tenant filter into statement AST (accepts str or Stmt) |
| `tokenize(input)` | Tokenize QQL string for syntax highlighting or inspection |
| `compile_query(input)` | Lower QQL statement into typed `{ method, path, payload }` route dict |
| `explain(query)` | Inspect the execution plan without executing network calls (accepts str or Stmt) |
| `execute(query, ..., on_error="stop")` | One-shot execute with a temporary edge client |
| `execute_async(query, ..., on_error="stop")` | Async variant of execute |
| `__version__` | Package runtime version string |

## Documentation Links

- [QQL Syntax Guide](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md)
- [Filter Documentation](https://github.com/srimon12/qql-rs/blob/main/docs/filters.md)
- [Filter Injection Guide](https://github.com/srimon12/qql-rs/blob/main/docs/inject_filter.md)
- [Changelog](https://github.com/srimon12/qql-rs/blob/main/CHANGELOG.md)
