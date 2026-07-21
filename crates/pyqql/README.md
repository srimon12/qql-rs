# pyqql

Native Python bindings for the Qdrant Query Language (QQL) parser, router, and execution engine, compiled with PyO3.

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **Automated Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI) for text-to-vector search
- **Zero-Copy Lowering**: Lower QQL queries to Qdrant OpenAPI REST routes via `compile_query`
- **Native parsing**: Rust-speed QQL parsing in Python returning typed `Stmt` objects or Python dictionaries
- **Filter injection**: Add tenant isolation filters programmatically
- **Validation**: Check if a query string is valid QQL

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

# Explain query execution plan
plan = client.explain("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
print(plan)

# 2. Pure AST Parsing & Filter Injection
stmt = pyqql.parse("QUERY 'vector database' FROM docs USING dense LIMIT 10")
valid = pyqql.is_valid("QUERY 'test' FROM docs")
secured_stmt = pyqql.inject_filter("QUERY 'patients' FROM medical LIMIT 5", "org_id", "=", "acme-corp")
```

## API Summary

| Class / Function | Description |
|---|---|
| `Client(url, api_key, use_grpc, embedder)` | Client for executing QQL against a live Qdrant database |
| `HttpEmbedder(endpoint, model, dimension, api_key)` | First-class HTTP embedding provider configuration |
| `parse(input)` | Parse single statement to typed `Stmt` object |
| `parse_all(input)` | Parse semicolon-delimited script into a list of `Stmt` objects |
| `parse_batch(queries)` | Batch-parse multiple query strings |
| `is_valid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Programmatically inject tenant filter into statement AST |
| `tokenize(input)` | Tokenize QQL string for syntax highlighting or inspection |
| `compile_query(input)` | Lower QQL statement into typed `{ method, path, payload }` route dict |
| `explain(query)` | Inspect the execution plan without executing network calls |
