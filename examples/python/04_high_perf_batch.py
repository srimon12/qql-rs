"""04 High-Performance: Script parsing and batch parsing for throughput."""
import pyqql

script = """
    CREATE COLLECTION docs HYBRID;
    INSERT INTO docs VALUES {id: 1, text: 'first'};
    INSERT INTO docs VALUES {id: 2, text: 'second'};
    QUERY 'test' FROM docs LIMIT 10;
"""
stmts = pyqql.parse_all(script)
print("=== Script Parsing (parse_all) ===")
print(f"Parsed {len(stmts)} statements from a .qql script:")
for i, s in enumerate(stmts):
    print(f"  [{i}] {s[:80]}...")

queries = [
    "QUERY 'alpha' FROM docs LIMIT 5",
    "QUERY 'beta'  FROM docs LIMIT 5",
    "QUERY 'gamma' FROM docs LIMIT 5",
]
results = pyqql.parse_batch(queries)
print("\n=== Batch Parsing (parse_batch) ===")
print(f"Parsed {len(results)} queries in a single FFI call:")
for i, r in enumerate(results):
    print(f"  [{i}] {r[:80]}...")
