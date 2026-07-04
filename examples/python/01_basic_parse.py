"""01 Basic: Parse, tokenize, and validate QQL queries."""
import pyqql

ast = pyqql.parse("CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)")
print("=== Parsed AST ===")
print(ast[:500])

print("\n=== Tokens ===")
for t in pyqql.tokenize("QUERY 'vector database' FROM docs LIMIT 10"):
    print(f"  {t['kind']:12} {t['text']!r:30}  pos={t['pos']}")

print("\n=== Validation ===")
for q in [
    "QUERY 'hello' FROM docs LIMIT 5",
    "CREATE COLLECTION docs",
    "SELECT * FROM docs WHERE id = 1",
    "",
    "BOGUS STUFF",
]:
    print(f"  valid={pyqql.is_valid(q):5}  {q!r}")
