"""Expert: Multi-tenant query gateway — inject tenant_id as an auth middleware.

In production, this runs in an API gateway before forwarding the query to Qdrant.
Every user query gets the tenant_id injected — no per-tenant collections needed."""
import pyqql

USERS = {
    "alice":  {"tenant": "acme",  "role": "admin"},
    "bob":    {"tenant": "acme",  "role": "viewer"},
    "charlie":{"tenant": "globex","role": "viewer"},
}

TENANT_POLICY = ("tenant_id", "=")

def enforce(user: str, query: str) -> str:
    ctx = USERS.get(user)
    if not ctx:
        raise PermissionError("unknown user")
    if not pyqql.is_valid(query):
        raise ValueError("invalid QQL query")
    return pyqql.inject_filter(query, *TENANT_POLICY, '{"str": "%s"}' % ctx["tenant"])

requests = [
    ("alice",   "QUERY 'sales data' FROM analytics LIMIT 10"),
    ("bob",     "QUERY 'sales data' FROM analytics LIMIT 10"),
    ("charlie", "QUERY 'engineering docs' FROM docs LIMIT 5"),
]

print("=== QQL Query Gateway ===")
for user, raw_query in requests:
    safe = enforce(user, raw_query)
    print(f"\n  user={user:8} role={USERS[user]['role']:7}")
    print(f"  raw:  {raw_query}")
    print(f"  safe: {safe[:130]}...")
