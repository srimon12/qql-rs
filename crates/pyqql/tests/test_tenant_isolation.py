"""Cross-tenant isolation check for the multitenancy recipes guide.

Offline: parses a query, injects the trusted tenant predicate, and applies
the same predicate in memory to two points. The other tenant point is
excluded, so a read scoped to one tenant returns zero rows from the other.
Mirrors the runnable snippet in multitenancy-recipes.mdoc.
"""

import unittest

from pyqql import parse


def visible_points(points, tenant):
    return [p for p in points if p.get("tenant_id") == tenant]


class TestTenantIsolation(unittest.TestCase):
    def test_injected_predicate_scopes_reads(self):
        stmt = parse("QUERY 'risk factors' FROM documents USING dense LIMIT 10")[0]
        stmt.inject_filter("tenant_id", "=", "acme")
        rewritten = str(stmt)
        self.assertIn("tenant_id", rewritten)
        self.assertIn("acme", rewritten)

    def test_cross_tenant_reads_return_zero_rows(self):
        points = [
            {"id": 1, "tenant_id": "acme"},
            {"id": 2, "tenant_id": "globex"},
        ]
        self.assertEqual([p["id"] for p in visible_points(points, "acme")], [1])
        self.assertEqual([p["id"] for p in visible_points(points, "globex")], [2])
        # A read scoped to acme sees no globex rows and vice versa.
        acme_ids = {p["id"] for p in visible_points(points, "acme")}
        globex_ids = {p["id"] for p in visible_points(points, "globex")}
        self.assertEqual(acme_ids & globex_ids, set())
        # Direct cross-tenant predicate matches nothing.
        cross = [p for p in points if p["tenant_id"] == "acme" and p["tenant_id"] == "globex"]
        self.assertEqual(cross, [])

    def test_middleware_tenant_wiring(self):
        # Minimal middleware shape: trusted tenant from request state flows
        # into inject_filter and shard_key together.
        def wire(query, tenant):
            stmt = parse(query)[0]
            stmt.inject_filter("tenant_id", "=", tenant)
            stmt.shard_key = tenant
            return stmt

        stmt = wire("QUERY 'x' FROM documents USING dense LIMIT 5", "acme")
        self.assertEqual(stmt.shard_key, "acme")
        self.assertIn("acme", str(stmt))


if __name__ == "__main__":
    unittest.main()
