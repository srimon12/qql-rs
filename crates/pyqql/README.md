# pyqql

Native Python bindings for the Qdrant Query Language (QQL) parser, compiled with PyO3.

## Features

- **Native parsing**: Rust-speed QQL parsing in Python
- **AST dictionaries**: Parsed queries as typed Python dicts
- **Filter injection**: Add tenant isolation filters to parsed ASTs
- **Validation**: Check if a query string is valid QQL
- **Batch parsing**: Parse multiple queries at once

## Installation

```bash
pip install pyqql
```

## Usage

```python
import pyqql

# Parse to AST dict
ast = pyqql.parse("QUERY 'vector database' FROM docs LIMIT 10")
print(ast)

# Parse multiple statements
stmts = pyqql.parse_all("INSERT INTO docs ...; QUERY 'text' FROM docs ...")

# Parse batch (list of queries)
results = pyqql.parse_batch([
    "QUERY 'ml' FROM docs LIMIT 5",
    "QUERY 'nlp' FROM docs LIMIT 5",
])

# Validate without parsing
valid = pyqql.is_valid("SELECT * FROM docs WHERE id = 1")

# Inject security filter into AST
stmt = pyqql.parse("QUERY 'patients' FROM medical LIMIT 5")
pyqql.inject_filter(stmt, "org_id", "=", "acme-corp")

# Tokenize query string
tokens = pyqql.tokenize("QUERY 'hello' FROM docs LIMIT 5")
```

## API

| Function | Returns | Description |
|---|---|---|
| `parse(input)` | `dict` | Parse single statement → AST dict |
| `parse_all(input)` | `list[dict]` | Parse multiple semicolon-separated statements |
| `parse_batch(queries)` | `list[dict]` | Parse a list of query strings |
| `is_valid(input)` | `bool` | Check if query string is valid QQL |
| `inject_filter(query, field, op, value)` | `str` | Inject filter into query AST |
| `tokenize(input)` | `str` | Tokenize query string |
