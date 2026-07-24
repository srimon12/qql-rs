# pyqql-edge

Local QQL execution for Python — qdrant-edge + fastembed, zero network.

```python
import pyqql_edge

# Parser (same API as pyqql)
stmt = pyqql_edge.parse("QUERY 'hello' FROM docs LIMIT 10")[0]
tokens = pyqql_edge.tokenize("QUERY 'test' FROM docs")

# Discover local ONNX models
models = pyqql_edge.list_embedding_models()
# [{'name': 'BGESmallENV15', 'model_code': 'Xenova/bge-small-en-v1.5', 'dim': 384, ...}, ...]

# Edge execution — pick model (default BGESmallENV15 / 384-d)
exec = pyqql_edge.local_executor(
    "./qdrant_data",
    on_disk_payload=False,
    model="bge-small-en-v1.5",        # enum name, HF code, or short alias
    cache_dir="/var/cache/fastembed", # optional
)

# Schema-aware text auto-embed (dense-only, sparse-only, and hybrid)
exec.execute("CREATE COLLECTION docs HYBRID")
exec.execute(
    'UPSERT INTO docs VALUES {id: "550e8400-e29b-41d4-a716-446655440001", text: "hello"}'
)
result = exec.execute("QUERY 'hello' FROM docs USING dense LIMIT 10", on_error="stop")
```

`execute()` and `execute_async()` return the same `ExecutionReport` dict as
`pyqql`: `ok`, ordered `results`, `succeeded`, and `failed`.

## Edge gotchas

| Gotcha | Reality |
|--------|---------|
| Point IDs | Integers or UUIDs only — `"doc-1"` is rejected |
| Text UPSERT into an existing collection | Auto-embedding follows the schema: dense-only gets dense, sparse-only gets sparse, hybrid gets both |
| `QUERY 'text'` on HYBRID | Automatically selects the only dense vector; ambiguous topologies still require `USING` |
| `GROUP BY` / shard keys | Rejected clearly; never silently ignored in edge mode |
| Model locked at `local_executor()` | `USING MODEL 'other'` mismatches fail |
| Client lifetime | Call `close()` (or use Python `with Client`) before deleting `data_dir` |
