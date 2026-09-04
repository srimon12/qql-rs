import json
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
        res = pyqql.explain(query)
        self.assertTrue(res["ok"])
        self.assertIn("Statement: QUERY", res["plan"])
        self.assertIn("Collection: docs", res["plan"])

    def test_client_instantiation(self):
        client = pyqql.Client("http://localhost:6333", use_grpc=False)
        res = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertTrue(res["ok"])
        self.assertIn("Collection: docs", res["plan"])

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
        res = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertTrue(res["ok"])
        self.assertIn("Statement: QUERY", res["plan"])

    def test_client_dict_embedder(self):
        client = pyqql.Client(
            "http://localhost:6333",
            embedder={
                "endpoint": "http://localhost:11434/v1/embeddings",
                "model": "nomic-embed-text",
                "dimension": 768,
            },
        )
        res = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertTrue(res["ok"])
        self.assertIn("Statement: QUERY", res["plan"])

    def test_client_dict_embedder_with_rerank(self):
        """RT-05: remote embedder config with rerank_* fields accepted."""
        client = pyqql.Client(
            "http://localhost:6333",
            embedder={
                "endpoint": "http://localhost:11434/v1/embeddings",
                "model": "nomic-embed-text",
                "dimension": 768,
                "rerank_endpoint": "http://localhost:11434/rerank",
                "rerank_api_key": "rk-key",
                "rerank_model": "test-reranker",
            },
        )
        res = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertTrue(res["ok"])
        self.assertIn("Statement: QUERY", res["plan"])

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

    def test_client_route_affinity(self):
        """Route affinity (Qdrant 1.19) is accepted at construction and readable."""
        client = pyqql.Client(
            "http://localhost:6333",
            use_grpc=False,
            route_affinity="session-acme-42",
        )
        self.assertEqual(client.route_affinity, "session-acme-42")
        res = client.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertTrue(res["ok"])

        # Empty string is treated as unset (matches the Rust client contract).
        unset = pyqql.Client(
            "http://localhost:6333", use_grpc=False, route_affinity=""
        )
        self.assertIsNone(unset.route_affinity)
        self.assertIsNone(pyqql.Client("http://localhost:6333").route_affinity)

    def test_parse_json(self):
        raw = pyqql.parse_json("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIsInstance(raw, str)
        stmts = json.loads(raw)
        self.assertEqual(len(stmts), 1)
        self.assertEqual(stmts[0]["Query"]["collection"]["Explicit"], "docs")

    def test_client_compile(self):
        """Client.compile mirrors module-level compile_query (parity with nqql)."""
        client = pyqql.Client("http://localhost:6333", use_grpc=False)
        route = client.compile("QUERY 'hello' FROM docs LIMIT 10")
        expected = pyqql.compile_query("QUERY 'hello' FROM docs LIMIT 10")
        self.assertEqual(route["stmt_type"], "query")
        self.assertEqual(route, expected)

        with self.assertRaises(Exception):
            client.compile("NOT A QUERY")

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

    def test_bind_named(self):
        q = "QUERY 'shoes' FROM products WHERE category = :cat AND price < :max_p"
        bound = pyqql.bind(q, {"cat": "sneakers", "max_p": 100})
        self.assertEqual(
            bound,
            "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND price < 100",
        )
        bound_named = pyqql.bind(q, {"cat": "boots", "max_p": 50})
        self.assertEqual(
            bound_named,
            "QUERY 'shoes' FROM products WHERE category = 'boots' AND price < 50",
        )

    def test_bind_positional(self):
        q = "QUERY 'shoes' FROM products WHERE category = ? AND in_stock = ?"
        bound = pyqql.bind(q, ["sneakers", True])
        self.assertEqual(
            bound,
            "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND in_stock = true",
        )
        bound_pos = pyqql.bind(q, ["boots", False])
        self.assertEqual(
            bound_pos,
            "QUERY 'shoes' FROM products WHERE category = 'boots' AND in_stock = false",
        )

    def test_bind_bool_ordering(self):
        q = "WHERE flag = ? AND count = ?"
        bound = pyqql.bind(q, [True, 1])
        self.assertEqual(bound, "WHERE flag = true AND count = 1")

    def test_bind_dict_keys_and_floats(self):
        q = "UPSERT INTO t VALUES :doc"
        bound = pyqql.bind(q, {"doc": {"a: 1, b": 5, "rate": 1.25e-5}})
        self.assertIn("'a: 1, b': 5", bound)
        self.assertIn("1.25e-5", bound)

    def test_bind_invalid_float(self):
        q = "WHERE score > ?"
        with self.assertRaises(ValueError):
            pyqql.bind(q, [float("nan")])
        with self.assertRaises(ValueError):
            pyqql.bind(q, [float("inf")])

    def test_bind_mixed_style_error(self):
        with self.assertRaises(ValueError):
            pyqql.bind("WHERE x = ?", {"x": 1})
        with self.assertRaises(ValueError):
            pyqql.bind("WHERE x = :name", [1])

    def test_version_exported(self):
        self.assertTrue(hasattr(pyqql, "__version__"))
        self.assertIsInstance(pyqql.__version__, str)

    def test_tokenize_spans(self):
        tokens = pyqql.tokenize("QUERY 'test' FROM docs")
        self.assertGreater(len(tokens), 0)
        t0 = tokens[0]
        self.assertIn("kind", t0)
        self.assertIn("text", t0)
        self.assertIn("pos", t0)
        self.assertIn("end", t0)
        self.assertIn("len", t0)
        self.assertEqual(t0["pos"], 0)
        self.assertEqual(t0["end"], 5)
        self.assertEqual(t0["len"], 5)

    def test_stmt_compile_route(self):
        stmts = pyqql.parse("QUERY 'hello' FROM docs LIMIT 10")
        self.assertEqual(len(stmts), 1)
        route = stmts[0].compile_route()
        self.assertEqual(route["method"], "POST")
        self.assertEqual(route["path"], "/collections/docs/points/query")
        self.assertIn("payload", route)

    def test_client_close_and_context_manager(self):
        client = pyqql.Client("http://localhost:6333", use_grpc=False)
        client.close()
        with pyqql.Client("http://localhost:6333", use_grpc=False) as c:
            self.assertIsNotNone(c)


if __name__ == "__main__":
    unittest.main()

