"""02 Medium: Programmatic WHERE injection — QQL's superpower."""
import pyqql

q = "QUERY 'machine learning transformer' FROM papers LIMIT 20"

s = pyqql.inject_filter(q, "tenant_id", "=", '{"str": "acme-corp"}')
print("=== String filter ===")
print(s[:400])

s = pyqql.inject_filter(q, "impact_factor", ">=", '{"float": 5.0}')
print("\n=== Numeric filter ===")
print(s[:400])

s = pyqql.inject_filter(q, "is_published", "=", '{"bool": true}')
print("\n=== Boolean filter ===")
print(s[:400])
