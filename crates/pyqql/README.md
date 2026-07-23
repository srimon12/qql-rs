# pyqql

Native Python bindings for the Qdrant Query Language (QQL) parser, router, and execution engine, compiled with PyO3 0.23.

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **Automated Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI) for text-to-vector search
- **Zero-Copy Route Lowering**: Lower QQL queries to typed `{ method, path, payload }` route dicts via `compile_query`
- **Native parsing**: Rust-speed QQL parsing in Python returning typed `Stmt` objects or Python dicts
- **Filter injection**: Add tenant isolation filters programmatically
- **Smart batching**: Auto-batches contiguous same-collection query/mutation statements into single network calls
- **Shard key**: Read/write the shard key on QUERY, COUNT, SCROLL, UPSERT, and DELETE statements
- **Validation**: Check if a query string is valid QQL

## Limitations

- **Python <= 3.13**: PyO3 0.23 without ABI3 requires Python ≤ 3.13. Use a virtualenv with Python 3.12 or 3.13.
- **gRPC**: gRPC transport is available when building with `--features grpc`. The default build uses REST only.

## Installation

```bash
pip install pyqql
```

## Quick Start

```python
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

# Async variant
future = client.execute_async("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")

# Explain query execution plan
plan = client.explain("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
print(plan)

# 2. Pure AST Parsing & Filter Injection
stmt = pyqql.parse("QUERY 'vector database' FROM docs USING dense LIMIT 10")
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

## API Summary

| Export | Description |
|---|---|
| `Client(url, api_key, use_grpc, embedder)` | Client for executing QQL against a live Qdrant database |
| `HttpEmbedder(endpoint, model, dimension, api_key)` | First-class HTTP embedding provider configuration |
| `Stmt` | Parsed statement object with `inject_filter()`, `to_json()`, `to_dict()`, `shard_key` property |
| `parse(input)` | Parse single statement to typed `Stmt` object |
| `parse_all(input)` | Parse semicolon-delimited script into a list of `Stmt` objects |
| `parse_batch(queries)` | Batch-parse multiple query strings |
| `is_valid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Inject tenant filter into statement AST (accepts str or Stmt) |
| `tokenize(input)` | Tokenize QQL string for syntax highlighting or inspection |
| `compile_query(input)` | Lower QQL statement into typed `{ method, path, payload }` route dict |
| `explain(query)` | Inspect the execution plan without executing network calls (accepts str or Stmt) |
| `execute(query, url, api_key, use_grpc, embedder)` | Free-function convenience execute |
| `execute_async(query, url, api_key, use_grpc, embedder)` | Free-function async execute |
| `Client.execute(query)` | Execute a string, Stmt, list[str], or list[Stmt] (auto-batched) |
| `Client.execute_async(query)` | Async variant of execute |
| `Client.explain(query)` | Inspect execution plan (accepts str or Stmt) |
