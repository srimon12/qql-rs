import unittest
import pyqql_edge


class TestPyQqlEdge(unittest.TestCase):
    def test_parse(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        stmt = pyqql_edge.parse(query)
        self.assertTrue(hasattr(stmt, "to_dict"), "parse() should return a Stmt object")
        d = stmt.to_dict()
        self.assertIn("Query", d)
        self.assertEqual(d["Query"]["collection"]["Explicit"], "docs")
        self.assertEqual(
            d["Query"]["expression"]["Nearest"]["input"]["Text"]["text"], "hello"
        )

    def test_explain(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        plan = pyqql_edge.explain(query)
        self.assertIn("Statement: QUERY", plan)
        self.assertIn("Collection: docs", plan)

    def test_parse_batch(self):
        queries = ["QUERY 'test' FROM users LIMIT 5", "CREATE COLLECTION items"]
        results = pyqql_edge.parse_batch(queries)
        self.assertEqual(len(results), 2)
        d0 = results[0].to_dict()
        d1 = results[1].to_dict()
        self.assertEqual(d0["Query"]["collection"]["Explicit"], "users")
        self.assertIn("CreateCollection", d1)

    def test_tokenize(self):
        tokens = pyqql_edge.tokenize("QUERY 'test' FROM docs")
        self.assertTrue(len(tokens) > 0)
        self.assertEqual(tokens[0]["text"], "QUERY")

    def test_is_valid(self):
        self.assertTrue(pyqql_edge.is_valid("QUERY 'test' FROM docs LIMIT 5"))
        self.assertFalse(pyqql_edge.is_valid("garbage"))

    def test_compile(self):
        result = pyqql_edge.compile_query("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIsInstance(result, dict)
        self.assertEqual(result["method"], "POST")

    def test_invalid(self):
        with self.assertRaises(SyntaxError):
            pyqql_edge.parse("invalid syntax")

    def test_local_executor(self):
        import tempfile, os
        with tempfile.TemporaryDirectory() as tmpdir:
            exec = pyqql_edge.local_executor(tmpdir, on_disk_payload=False)
            self.assertIsInstance(exec, pyqql_edge.Client)
            plan = exec.explain("QUERY 'hello' FROM docs LIMIT 10")
            self.assertIn("Statement: QUERY", plan)


if __name__ == "__main__":
    unittest.main()
