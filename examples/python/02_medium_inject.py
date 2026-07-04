"""Medium: Programmatic WHERE injection — apply security filters to queries."""
import pyqql

# A user-provided search query — validate it first
user_query = "QUERY 'machine learning transformer' FROM papers LIMIT 20"
print(f"User query valid: {pyqql.is_valid(user_query)}")

# Inject a tenant_id filter (string value)
tenant_query = pyqql.inject_filter(
    user_query, "tenant_id", "=", '{"str": "acme-corp"}'
)
print("\n=== Tenant isolation ===")
print(tenant_query[:500])

# Inject a numeric threshold
boosted = pyqql.inject_filter(
    user_query, "impact_factor", ">=", '{"float": 5.0}'
)
print("\n=== Numeric threshold ===")
print(boosted[:500])

# Inject a boolean flag
published = pyqql.inject_filter(
    user_query, "is_published", "=", '{"bool": true}'
)
print("\n=== Boolean filter ===")
print(published[:500])
