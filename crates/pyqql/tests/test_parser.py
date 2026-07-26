import unittest
import pyqql


class TestPyQql(unittest.TestCase):
    def test_parse(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        stmts = pyqql.parse(query)
        self.assertIsInstance(stmts, list)
        self.assertEqual(len(stmts), 1)
        stmt = stmts[0]
        self.assertTrue(hasattr(stmt, "to_dict"), "parse() should return a Stmt object")
        d = stmt.to_dict()
        self.assertIn("Query", d)
        self.assertEqual(d["Query"]["collection"]["Explicit"], "docs")
        self.assertEqual(
            d["Query"]["expression"]["Nearest"]["input"]["Text"]["text"], "hello"
        )

    def test_explain(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        plan = pyqql.explain(query)
        self.assertIn("Statement: QUERY", plan)
        self.assertIn("Collection: docs", plan)

    def test_client_instantiation(self):
        client = pyqql.Client("http://localhost:6333", use_grpc=False)
        plan = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIn("Collection: docs", plan)

    def test_client_first_class_embedder(self):
        embedder = pyqql.HttpEmbedder(
            endpoint="http://localhost:11434/v1/embeddings",
            model="nomic-embed-text",
            dimension=768,
            api_key="embed-key",
        )
        client = pyqql.Client(
            "http://localhost:6333", api_key="test-key", embedder=embedder
        )
        plan = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIn("Statement: QUERY", plan)

    def test_client_dict_embedder(self):
        client = pyqql.Client(
            "http://localhost:6333",
            embedder={
                "endpoint": "http://localhost:11434/v1/embeddings",
                "model": "nomic-embed-text",
                "dimension": 768,
            },
        )
        plan = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIn("Statement: QUERY", plan)

    def test_embedder_validation(self):
        with self.assertRaisesRegex(ValueError, "model is required"):
            pyqql.HttpEmbedder(
                endpoint="http://localhost:11434/v1/embeddings",
                model="",
                dimension=768,
            )
        with self.assertRaisesRegex(ValueError, "embedder.dimension is required"):
            pyqql.Client(
                "http://localhost:6333",
                embedder={
                    "endpoint": "http://localhost:11434/v1/embeddings",
                    "model": "nomic-embed-text",
                },
            )

    def test_parse_script(self):
        results = pyqql.parse(
            "QUERY 'test' FROM users LIMIT 5; CREATE COLLECTION items"
        )
        self.assertEqual(len(results), 2)
        d0 = results[0].to_dict()
        d1 = results[1].to_dict()
        self.assertEqual(d0["Query"]["collection"]["Explicit"], "users")
        self.assertIn("CreateCollection", d1)

    def test_tokenize(self):
        tokens = pyqql.tokenize("QUERY 'test' FROM docs")
        self.assertTrue(len(tokens) > 0)
        self.assertEqual(tokens[0]["text"], "QUERY")

    def test_invalid(self):
        with self.assertRaises(SyntaxError):
            pyqql.parse("invalid syntax")

    def test_invalid_on_error(self):
        client = pyqql.Client("http://localhost:6333", use_grpc=False)
        with self.assertRaises(ValueError):
            client.execute("SHOW COLLECTIONS", on_error="typo")
        report = client.execute("invalid syntax", on_error="continue")
        self.assertFalse(report["ok"])
        self.assertEqual(report["failed"], 1)
        self.assertEqual(report["results"][0]["operation"], "PARSE")

    def test_invalid_filter_operator(self):
        stmt = pyqql.parse("QUERY 'hello' FROM docs LIMIT 10")[0]
        with self.assertRaisesRegex(SyntaxError, "unsupported comparison operator"):
            stmt.inject_filter("tenant_id", "contains", "acme")
        with self.assertRaisesRegex(SyntaxError, "unsupported comparison operator"):
            pyqql.inject_filter(
                "QUERY 'hello' FROM docs LIMIT 10",
                "tenant_id",
                "contains",
                "acme",
            )


if __name__ == "__main__":
    unittest.main()
