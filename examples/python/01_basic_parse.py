"""Basic: Parse, tokenize, and validate QQL queries."""
import pyqql

# Parse a CREATE COLLECTION statement
ast = pyqql.parse("CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)")
print("=== Parsed AST ===")
print(ast[:500])
print()

# Tokenize a QUERY
tokens = pyqql.tokenize("QUERY 'vector database' FROM docs LIMIT 10")
print("=== Tokens ===")
for t in tokens:
    print(f"  {t['kind']:12} {t['text']!r:30}  pos={t['pos']}")
print()

# Validate queries
for q in [
    "QUERY 'hello' FROM docs LIMIT 5",
    "CREATE COLLECTION docs",
    "SELECT * FROM docs WHERE id = 1",
    "",
    "BOGUS STUFF",
]:
    valid = pyqql.is_valid(q)
    print(f"  valid={valid}  {q!r}")
