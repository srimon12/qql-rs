# pyqql-edge

Local QQL execution for Python — qdrant-edge + fastembed, zero network.

```python
import pyqql_edge

# Parser (same API as pyqql)
stmt = pyqql_edge.parse("QUERY 'hello' FROM docs LIMIT 10")
tokens = pyqql_edge.tokenize("QUERY 'test' FROM docs")

# Edge execution (in-process HNSW)
exec = pyqql_edge.local_executor("./qdrant_data")
result = exec.execute("QUERY 'hello' FROM docs LIMIT 10")
```
