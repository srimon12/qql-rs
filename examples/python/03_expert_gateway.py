"""03 Expert: Multi-tenant query gateway using inject_filter as auth middleware."""
import pyqql

USERS = {
    "alice":  {"tenant": "acme",  "role": "admin"},
    "bob":    {"tenant": "acme",  "role": "viewer"},
    "charlie":{"tenant": "globex","role": "viewer"},
}

def enforce(user, query):
    ctx = USERS[user]
    safe = pyqql.inject_filter(query, "tenant_id", "=", ctx["tenant"])
    if ctx["role"] == "viewer":
        safe = pyqql.inject_filter(safe, "status", "!=", "confidential")
    return safe

requests = [
    ("alice",   "QUERY 'sales data' FROM analytics LIMIT 10"),
    ("bob",     "QUERY 'sales data' FROM analytics LIMIT 10"),
    ("charlie", "QUERY 'engineering docs' FROM docs LIMIT 5"),
]

print("=== QQL Query Gateway ===")
for user, raw in requests:
    safe = enforce(user, raw)
    print(f"\n  user={user:8} role={USERS[user]['role']:7}")
    print(f"  raw:  {raw}")
    print(f"  safe: {safe[:130]}...")
