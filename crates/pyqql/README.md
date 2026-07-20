# pyqql

Native Python bindings for the Qdrant Query Language (QQL) parser and execution engine, compiled with PyO3.

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **First-Class Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI)
- **Fast Dictionary Exports**: Zero-copy Python dictionary conversion via `pythonize`
- **Native parsing**: Rust-speed QQL parsing in Python
- **Filter injection**: Add tenant isolation filters to parsed ASTs
- **Validation**: Check if a query string is valid QQL

## Installation

```bash
pip install pyqql
```

## Quick Start

```python
import pyqql

# 1. Connect to live Qdrant with optional custom embedding provider
embedder = pyqql.HttpEmbedder(
    endpoint="http://localhost:11434/v1/embeddings",
    model="nomic-embed-text",
    dimension=768,
    api_key="optional-key"
)

client = pyqql.Client(
    url="http://localhost:6333",
    api_key="optional-qdrant-secret",
    use_grpc=False,
    embedder=embedder
)

# Execute QQL query
result = client.execute("QUERY 'cardiology' FROM medical_records LIMIT 5")
print(result)

# Explain query execution plan
plan = client.explain("QUERY 'test' FROM docs LIMIT 5")
print(plan)

# 2. Pure AST Parsing & Filter Injection
ast = pyqql.parse("QUERY 'vector database' FROM docs LIMIT 10")
valid = pyqql.is_valid("SELECT * FROM docs WHERE id = 1")
secured = pyqql.inject_filter("QUERY 'patients' FROM medical LIMIT 5", "org_id", "=", "acme-corp")
```

## API Summary

| Class / Function | Description |
|---|---|
| `Client(url, api_key, use_grpc, embedder)` | Client for executing QQL against a live Qdrant database |
| `HttpEmbedder(endpoint, model, dimension, api_key)` | First-class HTTP embedding provider configuration |
| `execute(query, url, api_key, use_grpc, embedder)` | One-off helper function to execute a QQL statement |
| `explain(query)` | Inspect the execution plan without executing network calls |
| `parse(input)` | Parse single statement to AST dictionary |
| `is_valid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Inject tenant filter into statement AST |
