#!/usr/bin/env python3
"""
Comprehensive test suite for pyqql-edge.
"""

import json
import os
import shutil
import tempfile
import unittest

import pyqql_edge


# ============================================================================
# Category A: Package inspection
# ============================================================================

class TestPackageInspection(unittest.TestCase):
    """A: Verify all expected exports and class signatures."""

    def test_a1_all_expected_exports(self):
        expected = [
            "Client",
            "Stmt",
            "bind",
            "compile_query",
            "execute",
            "execute_async",
            "explain",
            "inject_filter",
            "is_valid",
            "local_executor",
            "parse",
            "parse_json",
            "tokenize",
            "__version__",
        ]
        actual = dir(pyqql_edge)
        missing = [name for name in expected if name not in actual]
        self.assertEqual(missing, [], f"Missing exports: {missing}")

    def test_a2_stmt_is_class_with_methods(self):
        self.assertTrue(isinstance(pyqql_edge.Stmt, type))
        for attr in ("to_dict", "to_json", "inject_filter", "shard_key"):
            self.assertTrue(hasattr(pyqql_edge.Stmt, attr), f"Stmt missing {attr}")


# ============================================================================
# Category B: Parse API
# ============================================================================

class TestParseAPI(unittest.TestCase):
    """B: Test parse, to_dict, to_json, is_valid, tokenize, compile_query."""

    def test_b1_parse_simple_query(self):
        stmts = pyqql_edge.parse('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(stmts, list)
        self.assertEqual(len(stmts), 1)
        self.assertIsInstance(stmts[0], pyqql_edge.Stmt)

    def test_b2_parse_count(self):
        stmts = pyqql_edge.parse("COUNT FROM docs")
        self.assertEqual(len(stmts), 1)
        self.assertIsInstance(stmts[0], pyqql_edge.Stmt)

    def test_b3_parse_multi_statement_script(self):
        stmts = pyqql_edge.parse("COUNT FROM docs; COUNT FROM sec10k")
        self.assertEqual(len(stmts), 2)

    def test_b4_parse_show_collections(self):
        stmts = pyqql_edge.parse("SHOW COLLECTIONS")
        self.assertEqual(len(stmts), 1)

    def test_b5_to_dict_query_structure(self):
        stmts = pyqql_edge.parse('QUERY "hello" FROM docs LIMIT 5')
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("Query", d)
        self.assertEqual(d["Query"]["collection"]["Explicit"], "docs")

    def test_b6_to_dict_count_structure(self):
        stmts = pyqql_edge.parse("COUNT FROM docs")
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("Count", d)
        self.assertEqual(d["Count"]["collection"], {"Explicit": "docs"})

    def test_b7_show_collections_to_dict(self):
        stmts = pyqql_edge.parse("SHOW COLLECTIONS")
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("ShowCollections", d)

    def test_b8_to_json_returns_valid_json(self):
        stmts = pyqql_edge.parse('QUERY "hello" FROM docs LIMIT 5')
        j = stmts[0].to_json()
        self.assertIsInstance(j, str)
        json.loads(j)

    def test_b9_is_valid_true(self):
        self.assertTrue(pyqql_edge.is_valid("COUNT FROM docs"))
        self.assertTrue(pyqql_edge.is_valid('QUERY "hello" FROM docs LIMIT 5'))

    def test_b10_is_valid_false(self):
        self.assertFalse(pyqql_edge.is_valid("SELECT * FROM docs"))

    def test_b11_tokenize(self):
        tokens = pyqql_edge.tokenize('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(tokens, list)
        self.assertGreater(len(tokens), 0)

    def test_b12_compile_query(self):
        cq = pyqql_edge.compile_query('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(cq, dict)
        self.assertEqual(cq["method"], "POST")

    def test_b13_client_compile_parity(self):
        """Client.compile mirrors module-level compile_query (parity with pyqql)."""
        executor = pyqql_edge.local_executor(
            tempfile.mkdtemp(prefix="pyqql-edge-compile-"), False
        )
        try:
            route = executor.compile('QUERY "hello" FROM docs LIMIT 5')
            expected = pyqql_edge.compile_query('QUERY "hello" FROM docs LIMIT 5')
            self.assertIsInstance(route, dict)
            self.assertEqual(route, expected)
            self.assertEqual(route["stmt_type"], "query")
        finally:
            executor.close()


# ============================================================================
# Category C: Error handling & inject_filter
# ============================================================================

class TestErrorHandling(unittest.TestCase):
    def test_c1_parse_invalid_syntax(self):
        with self.assertRaises(SyntaxError):
            pyqql_edge.parse("INVALID SYNTAX @@@@")

    def test_c2_inject_filter_unsupported_op(self):
        with self.assertRaises(SyntaxError):
            pyqql_edge.inject_filter('QUERY "hello" FROM docs', "f", "contains", "v")

    def test_c3_inject_filter_valid_op(self):
        r = pyqql_edge.inject_filter('QUERY "hello" FROM docs', "tenant_id", "=", "acme")
        self.assertIsInstance(r, pyqql_edge.Stmt)
        d = r.to_dict()
        self.assertIsNotNone(d["Query"]["filter"])

    def test_c4_delete_payload_shard_key(self):
        stmt = pyqql_edge.parse(
            "DELETE PAYLOAD draft FROM docs WHERE status = 'archived'"
        )[0]
        stmt.shard_key = "tenant-a"
        self.assertEqual(stmt.shard_key, "tenant-a")
        self.assertEqual(
            stmt.to_dict()["DeletePayload"]["shard_key"],
            "tenant-a",
        )

    def test_c5_stmt_shard_key_property(self):
        stmts = pyqql_edge.parse("QUERY TEXT 'x' FROM docs SHARD 't' LIMIT 5")
        assert stmts[0].shard_key == "t"
        s = pyqql_edge.parse("QUERY TEXT 'x' FROM docs LIMIT 5")[0]
        s.shard_key = "acme"
        assert s.shard_key == "acme"


class TestLocalExecutor(unittest.TestCase):
    def setUp(self):
        self.test_dir = tempfile.mkdtemp(prefix="pyqql_edge_test_")

    def tearDown(self):
        shutil.rmtree(self.test_dir, ignore_errors=True)

    def test_d1_list_embedding_models(self):
        if hasattr(pyqql_edge, "list_embedding_models") and pyqql_edge.list_embedding_models is not None:
            models = pyqql_edge.list_embedding_models()
            self.assertIsInstance(models, list)
            self.assertGreater(len(models), 0)

    def test_d2_local_executor_flow(self):
        client = pyqql_edge.local_executor(self.test_dir, on_disk_payload=False)
        r1 = client.execute("CREATE COLLECTION edge_docs")
        self.assertTrue(r1["ok"])

        r2 = client.execute("SHOW COLLECTIONS")
        self.assertTrue(r2["ok"])

        r3 = client.execute("COUNT FROM edge_docs")
        self.assertTrue(r3["ok"])


# ============================================================================
# Category E: Parameter Binding
# ============================================================================

class TestParameterBinding(unittest.TestCase):
    """E: Test parameter binding (:name and ?)."""

    def test_e1_bind_named(self):
        q = "QUERY 'shoes' FROM products WHERE category = :cat AND price < :max_p"
        res = pyqql_edge.bind(q, {"cat": "sneakers", "max_p": 100})
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND price < 100",
        )

    def test_e2_bind_positional(self):
        q = "QUERY 'shoes' FROM products WHERE category = ? AND in_stock = ?"
        res = pyqql_edge.bind(q, ["sneakers", True])
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND in_stock = true",
        )

    def test_e3_bind_named_fn(self):
        q = "QUERY 'shoes' FROM products WHERE category = :cat"
        res = pyqql_edge.bind(q, {"cat": "boots"})
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'boots'",
        )

    def test_e4_bind_positional_fn(self):
        q = "QUERY 'shoes' FROM products WHERE category = ?"
        res = pyqql_edge.bind(q, ["boots"])
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'boots'",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)

