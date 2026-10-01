"""Cross-tenant isolation wiring for the multitenancy recipes guide.

Offline: parses queries and asserts the trusted tenant predicate and shard
key land in the rewritten statement. Mirrors the runnable snippet in
multitenancy-recipes.mdoc.
"""

import unittest

from pyqql import parse


class TestTenantIsolation(unittest.TestCase):
    def test_injected_predicate_scopes_reads(self):
        stmt = parse("QUERY 'risk factors' FROM documents USING dense LIMIT 10")[0]
        stmt.inject_filter("tenant_id", "=", "acme")
        rewritten = str(stmt)
        self.assertIn("tenant_id", rewritten)
        self.assertIn("acme", rewritten)

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
