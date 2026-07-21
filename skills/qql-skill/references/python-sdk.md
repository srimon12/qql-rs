# Python SDK (`pyqql`) Reference & Examples

Native Python bindings for QQL compiled with PyO3.

## Installation

```bash
pip install pyqql
```

## Quick Start & Client Configuration

```python
import pyqql

# 1. Initialize HTTP Embedder (for Ollama, OpenAI, vLLM, TEI)
embedder = pyqql.HttpEmbedder(
    endpoint="http://localhost:11434/v1/embeddings",
    model="all-minilm:l6-v2",
    dimension=384,
    api_key=""
)

# 2. Initialize QQL Client
client = pyqql.Client(
    url="http://localhost:6333",
    api_key=None,
    use_grpc=False,
    embedder=embedder
)

# 3. Execute QQL Queries
response = client.execute("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
print("Response data:", response)

# 4. Explain Execution Plan
plan = client.explain("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5")
print("Execution plan:", plan)
```

## AST Parsing, Compilation & Filter Injection

```python
import pyqql

# Parse query string into typed Stmt object
stmt = pyqql.parse("QUERY 'vector search' FROM docs USING dense LIMIT 10")

# Programmatically inject security or tenant filter into Stmt or query string
secured_stmt = pyqql.inject_filter(stmt, "tenant_id", "=", "tenant_acme")

# Check if query is valid QQL
if pyqql.is_valid("QUERY 'search' FROM docs"):
    print("Valid QQL statement")

# Compile statement to Qdrant REST route dictionary
route = pyqql.compile_query("QUERY 'search' FROM docs USING dense LIMIT 10")
print("Compiled route:", route["method"], route["path"], route["payload"])

# Tokenize QQL query string
tokens = pyqql.tokenize("QUERY 'search' FROM docs WHERE id = 1")
for t in tokens:
    print(t["kind"], t["text"], t["pos"])
```

## Batch Processing

```python
import pyqql

# Parse batch of queries in a single FFI boundary call
stmts = pyqql.parse_batch([
    "QUERY 'query 1' FROM docs USING dense LIMIT 5",
    "QUERY 'query 2' FROM docs USING dense LIMIT 5",
])
```
