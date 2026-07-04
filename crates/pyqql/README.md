# pyqql

Native Python bindings for the Qdrant Query Language (QQL) parser and execution engine, compiled with PyO3.

---

## Features
* **Native Speed**: Zero-overhead AST parsing and execution written in Rust.
* **Typing support**: Parses raw QQL query strings directly into typed Python dictionaries representing the query AST.
* **Recursive Security Filter Injection**: Easily inject tenant-isolation filters recursively into raw query AST structures.

---

## Installation

Install using maturin or pip:
```bash
pip install pyqql
```

---

## Usage

### Parsing QQL to Python Dict
```python
import pyqql

# Parse query string to raw AST representation
ast = pyqql.parse("QUERY 'vector database' FROM docs LIMIT 10")
print(ast)
```

### Mutating/Injecting filters in Python
```python
import pyqql

# Secure an incoming client query with org_id isolation
stmt = pyqql.parse("QUERY 'patients' FROM medical LIMIT 5")

# Injects 'org_id = acme-corp' into the AST recursively
pyqql.inject_filter(stmt, "org_id", "=", "acme-corp")
```
