import unittest
import pyqql


class TestPyQql(unittest.TestCase):
    def test_parse(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        stmt = pyqql.parse(query)
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

    def test_parse_batch(self):
        queries = ["QUERY 'test' FROM users LIMIT 5", "CREATE COLLECTION items"]
        results = pyqql.parse_batch(queries)
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


if __name__ == "__main__":
    unittest.main()
