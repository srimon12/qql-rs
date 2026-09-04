#!/usr/bin/env python3
"""
Comprehensive test suite for pyqql (fresh install from PyPI).

Covers:
  A. Package inspection
  B. Parse API
  C. Error handling
  D. inject_filter
  E. Client (remote, REST)
  F. HttpEmbedder
  G. Full E2E pipeline against live Qdrant
  H. Script-level execute
  I. compile_query route contract
  J. Edge cases
"""

import asyncio
import json
import unittest
import urllib.request
import uuid

import pyqql


def _qdrant_available():
    """Check if Qdrant is reachable at localhost:6333."""
    try:
        req = urllib.request.Request("http://localhost:6333/healthz")
        with urllib.request.urlopen(req, timeout=2) as resp:
            return resp.status == 200
    except Exception:
        return False


_qdrant_ok = _qdrant_available()
E2E_COLLECTION = "pyqql_fresh_e2e_test"


# ============================================================================
# Category A: Package inspection
# ============================================================================

class TestPackageInspection(unittest.TestCase):
    """A: Verify all expected exports and class signatures."""

    def test_a1_all_expected_exports(self):
        expected = [
            "Client",
            "HttpEmbedder",
            "Stmt",
            "bind",
            "parse",
            "is_valid",
            "inject_filter",
            "tokenize",
            "compile_query",
            "explain",
            "execute",
            "execute_async",
        ]
        actual = dir(pyqql)
        missing = [name for name in expected if name not in actual]
        self.assertEqual(missing, [], f"Missing exports: {missing}")

    def test_a2_stmt_is_class_with_methods(self):
        self.assertTrue(isinstance(pyqql.Stmt, type))
        for attr in ("to_dict", "to_json", "inject_filter", "shard_key"):
            self.assertTrue(hasattr(pyqql.Stmt, attr), f"Stmt missing {attr}")

    def test_a3_httpembedder_signature(self):
        he = pyqql.HttpEmbedder(
            endpoint="http://localhost:8080/v1/embeddings",
            model="text-embedding-3-small",
            dimension=1536,
            api_key="sk-test",
        )
        self.assertIsNotNone(he)

    def test_a4_client_class(self):
        self.assertTrue(isinstance(pyqql.Client, type))

    def test_a5_parse_is_function(self):
        self.assertTrue(callable(pyqql.parse))

    def test_a6_is_valid_is_function(self):
        self.assertTrue(callable(pyqql.is_valid))

    def test_a7_tokenize_is_function(self):
        self.assertTrue(callable(pyqql.tokenize))

    def test_a8_compile_query_is_function(self):
        self.assertTrue(callable(pyqql.compile_query))

    def test_a9_explain_is_function(self):
        self.assertTrue(callable(pyqql.explain))

    def test_a10_execute_functions(self):
        self.assertTrue(callable(pyqql.execute))
        self.assertTrue(callable(pyqql.execute_async))


# ============================================================================
# Category B: Parse API
# ============================================================================

class TestParseAPI(unittest.TestCase):
    """B: Test parse, to_dict, to_json, is_valid, tokenize, compile_query."""

    def test_b1_parse_simple_query(self):
        stmts = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(stmts, list)
        self.assertEqual(len(stmts), 1)
        self.assertIsInstance(stmts[0], pyqql.Stmt)

    def test_b2_parse_count(self):
        stmts = pyqql.parse("COUNT FROM docs")
        self.assertEqual(len(stmts), 1)
        self.assertIsInstance(stmts[0], pyqql.Stmt)

    def test_b3_parse_multi_statement_script(self):
        stmts = pyqql.parse("COUNT FROM docs; COUNT FROM sec10k")
        self.assertEqual(len(stmts), 2)
        for s in stmts:
            self.assertIsInstance(s, pyqql.Stmt)

    def test_b4_parse_show_collections(self):
        stmts = pyqql.parse("SHOW COLLECTIONS")
        self.assertEqual(len(stmts), 1)
        self.assertIsInstance(stmts[0], pyqql.Stmt)

    def test_b5_to_dict_query_structure(self):
        stmts = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("Query", d)
        q = d["Query"]
        self.assertEqual(q["collection"]["Explicit"], "docs")
        self.assertEqual(
            q["expression"]["Nearest"]["input"]["Text"]["text"],
            "hello",
        )
        self.assertEqual(q["page"]["limit"], 5)

    def test_b6_to_dict_count_structure(self):
        """to_dict for COUNT uses {Explicit:...} structure consistent with QUERY."""
        stmts = pyqql.parse("COUNT FROM docs")
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("Count", d)
        c = d["Count"]
        self.assertEqual(c["collection"], {"Explicit": "docs"})
        self.assertIsNone(c["filter"])

    def test_b7_show_collections_to_dict(self):
        """SHOW COLLECTIONS to_dict returns dict {'ShowCollections': {}}."""
        stmts = pyqql.parse("SHOW COLLECTIONS")
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("ShowCollections", d)

    def test_b8_to_json_returns_valid_json(self):
        stmts = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')
        j = stmts[0].to_json()
        self.assertIsInstance(j, str)
        json.loads(j)

    def test_b9_to_json_multi_statement(self):
        stmts = pyqql.parse("COUNT FROM docs; COUNT FROM sec10k")
        for s in stmts:
            j = s.to_json()
            self.assertIsInstance(j, str)
            json.loads(j)

    def test_b10_is_valid_true(self):
        self.assertTrue(pyqql.is_valid("COUNT FROM docs"))
        self.assertTrue(pyqql.is_valid('QUERY "hello" FROM docs LIMIT 5'))
        self.assertTrue(pyqql.is_valid("SHOW COLLECTIONS"))

    def test_b11_is_valid_false(self):
        self.assertFalse(pyqql.is_valid("SELECT * FROM docs"))
        # Plan-level invalidity counts as invalid too — is_valid applies the
        # same gate as execution and the language conformance suite.
        self.assertFalse(
            pyqql.is_valid(
                "QUERY VECTOR [0.1, 0.2] FROM docs USING lexical_v2 AS SPARSE LIMIT 10;"
            )
        )

    def test_b12_tokenize_returns_list_of_dicts(self):
        tokens = pyqql.tokenize('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(tokens, list)
        self.assertGreater(len(tokens), 0)
        t = tokens[0]
        self.assertIsInstance(t, dict)
        for key in ("kind", "text", "pos"):
            self.assertIn(key, t, f"Token missing key: {key}")

    def test_b13_compile_query_returns_route_dict(self):
        cq = pyqql.compile_query('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(cq, dict)
        for key in ("method", "path", "payload"):
            self.assertIn(key, cq, f"compile_query missing key: {key}")
        self.assertEqual(cq["method"], "POST")
        self.assertIn("/collections/docs", cq["path"])
        self.assertIsInstance(cq["payload"], dict)


# ============================================================================
# Category C: Error handling
# ============================================================================

class TestErrorHandling(unittest.TestCase):
    """C: Test error behavior for invalid inputs."""

    def test_c1_parse_invalid_syntax(self):
        with self.assertRaises(SyntaxError):
            pyqql.parse("NOT A VALID QQL!!! @@@@")

    def test_c2_inject_filter_invalid_operator(self):
        with self.assertRaises(SyntaxError) as ctx:
            pyqql.inject_filter(
                'QUERY "hello" FROM docs LIMIT 5',
                "field",
                "contains",
                "value",
            )
        self.assertIn("unsupported comparison operator", str(ctx.exception))

    def test_c3_stmt_inject_filter_invalid_operator(self):
        stmt = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')[0]
        with self.assertRaises(SyntaxError) as ctx:
            stmt.inject_filter("field", "contains", "value")
        self.assertIn("unsupported comparison operator", str(ctx.exception))

    def test_c4_client_execute_on_error_typo(self):
        client = pyqql.Client()
        with self.assertRaises(ValueError) as ctx:
            client.execute("SHOW COLLECTIONS", on_error="typo")
        self.assertIn("on_error", str(ctx.exception).lower())

    def test_c5_client_execute_on_error_continue_bad_syntax(self):
        client = pyqql.Client()
        result = client.execute("BROKEN !!! SYNTAX @@@@", on_error="continue")
        self.assertIsInstance(result, dict)
        self.assertFalse(result["ok"])
        self.assertEqual(result["failed"], 1)
        self.assertEqual(result["succeeded"], 0)
        self.assertEqual(len(result["results"]), 1)
        self.assertFalse(result["results"][0]["ok"])
        self.assertEqual(result["results"][0]["operation"], "PARSE")

    def test_c6_compile_query_empty_string(self):
        with self.assertRaises(SyntaxError):
            pyqql.compile_query("")

    def test_c7_inject_filter_empty_string(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter("", "field", "=", "value")


# ============================================================================
# Category D: inject_filter
# ============================================================================

class TestInjectFilter(unittest.TestCase):
    """D: Test inject_filter function and method with various operators."""

    def test_d1_inject_filter_standalone_returns_stmt(self):
        result = pyqql.inject_filter(
            'QUERY "hello" FROM docs LIMIT 5',
            "tenant_id",
            "=",
            "acme",
        )
        self.assertIsInstance(result, pyqql.Stmt)
        d = result.to_dict()
        f = d["Query"]["filter"]
        self.assertIsNotNone(f)
        self.assertEqual(f["Compare"]["field"], "tenant_id")
        self.assertEqual(f["Compare"]["op"], "Eq")
        self.assertEqual(f["Compare"]["value"]["Str"], "acme")

    def test_d2_inject_filter_does_not_mutate_original(self):
        original = 'QUERY "hello" FROM docs LIMIT 5'
        result = pyqql.inject_filter(original, "x", "=", "v")
        self.assertIsNotNone(result.to_dict()["Query"]["filter"])

    def test_d3_stmt_inject_filter_in_place(self):
        stmt = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')[0]
        self.assertIsNone(stmt.to_dict()["Query"]["filter"])
        stmt.inject_filter("tenant_id", "=", "acme")
        self.assertIsNotNone(stmt.to_dict()["Query"]["filter"])

    def test_d4_inject_into_already_filtered_query(self):
        result = pyqql.inject_filter(
            'QUERY "hello" FROM docs WHERE color = "red" LIMIT 5',
            "tenant_id",
            "=",
            "acme",
        )
        d = result.to_dict()
        f = d["Query"]["filter"]
        self.assertIn("And", f)
        self.assertEqual(len(f["And"]["operands"]), 2)

    def test_d5_operator_eq(self):
        r = pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "=", "v")
        self.assertEqual(r.to_dict()["Query"]["filter"]["Compare"]["op"], "Eq")

    def test_d6_operator_gt(self):
        r = pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", ">", "v")
        self.assertEqual(r.to_dict()["Query"]["filter"]["Compare"]["op"], "Gt")

    def test_d7_operator_lt(self):
        r = pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "<", "v")
        self.assertEqual(r.to_dict()["Query"]["filter"]["Compare"]["op"], "Lt")

    def test_d8_operator_gte(self):
        r = pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", ">=", "v")
        self.assertEqual(r.to_dict()["Query"]["filter"]["Compare"]["op"], "Gte")

    def test_d9_operator_lte(self):
        r = pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "<=", "v")
        self.assertEqual(r.to_dict()["Query"]["filter"]["Compare"]["op"], "Lte")

    def test_d10_operator_not_eq_not_supported(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "!=", "v")

    def test_d11_in_not_supported(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "in", "v")

    def test_d12_not_in_not_supported(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "not_in", "v")

    def test_d13_match_not_supported(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "match", "v")

    def test_d14_is_null_not_supported(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "is_null", None)

    def test_d15_is_not_null_not_supported(self):
        with self.assertRaises(SyntaxError):
            pyqql.inject_filter('QUERY "x" FROM docs LIMIT 1', "f", "is_not_null", None)


# ============================================================================
# Category E: Client (remote, REST)
# ============================================================================

@unittest.skipUnless(_qdrant_ok, "Qdrant not available at localhost:6333")
class TestClient(unittest.TestCase):
    """E: Test the Client class against live Qdrant."""

    def setUp(self):
        self.client = pyqql.Client(
            url="http://localhost:6333",
            api_key=None,
            use_grpc=False,
        )

    @classmethod
    def setUpClass(cls):
        cls.client = pyqql.Client(url="http://localhost:6333")
        try:
            cls.client.execute(f"CREATE COLLECTION {E2E_COLLECTION}")
        except Exception:
            pass

    @classmethod
    def tearDownClass(cls):
        try:
            cls.client.execute(f"DROP COLLECTION {E2E_COLLECTION}")
        except Exception:
            pass

    def test_e1_client_explain(self):
        result = self.client.explain('QUERY "hello" FROM docs LIMIT 5')
        self.assertIsInstance(result, dict)
        self.assertTrue(result["ok"])
        self.assertIn("plan", result)

    def test_e2_client_execute_show_collections(self):
        result = self.client.execute("SHOW COLLECTIONS")
        self.assertIsInstance(result, dict)
        self.assertTrue(result["ok"])
        self.assertEqual(result["failed"], 0)
        self.assertGreater(len(result["results"]), 0)
        r0 = result["results"][0]
        self.assertTrue(r0["ok"])
        self.assertEqual(r0["operation"], "SHOW_COLLECTIONS")

    def test_e3_client_execute_invalid_syntax_continue(self):
        result = self.client.execute(
            "INVALID !!! @@@@",
            on_error="continue",
        )
        self.assertIsInstance(result, dict)
        self.assertFalse(result["ok"])
        self.assertEqual(result["failed"], 1)
        self.assertEqual(result["results"][0]["operation"], "PARSE")

    def test_e4_client_execute_async_basic(self):
        async def _run():
            result = await self.client.execute_async(f"COUNT FROM {E2E_COLLECTION}")
            self.assertIsInstance(result, dict)
            self.assertTrue(result["ok"])
            return result

        result = asyncio.run(_run())
        self.assertIn("results", result)

    def test_e5_client_execute_count(self):
        result = self.client.execute(f"COUNT FROM {E2E_COLLECTION}")
        self.assertTrue(result["ok"])
        r0 = result["results"][0]
        self.assertEqual(r0["operation"], "COUNT")
        self.assertIn("data", r0)
        self.assertIn("count", r0["data"]["result"])

    def test_e6_client_execute_count_with_filter(self):
        """COUNT with WHERE filter works against live Qdrant."""
        result = self.client.execute(
            f'COUNT FROM {E2E_COLLECTION} WHERE symbol = "AAPL"'
        )
        self.assertTrue(result["ok"])
        r0 = result["results"][0]
        self.assertEqual(r0["operation"], "COUNT")
        self.assertIn("data", r0)
        count = r0["data"]["result"]["count"]
        self.assertIsInstance(count, int)

    def test_e7_module_level_execute(self):
        result = pyqql.execute(f"COUNT FROM {E2E_COLLECTION}")
        self.assertIsInstance(result, dict)
        self.assertTrue(result["ok"])

    def test_e8_module_level_execute_async(self):
        async def _run():
            result = await pyqql.execute_async(f"COUNT FROM {E2E_COLLECTION}")
            self.assertIsInstance(result, dict)
            self.assertTrue(result["ok"])

        asyncio.run(_run())


# ============================================================================
# Category F: HttpEmbedder
# ============================================================================

class TestHttpEmbedder(unittest.TestCase):
    """F: Test HttpEmbedder construction and integration with Client."""

    def test_f1_construct_with_all_params(self):
        he = pyqql.HttpEmbedder(
            endpoint="http://localhost:8080/v1/embeddings",
            model="text-embedding-3-small",
            dimension=1536,
            api_key="sk-test-key",
        )
        self.assertIsNotNone(he)

    def test_f2_construct_without_api_key(self):
        he = pyqql.HttpEmbedder(
            endpoint="http://localhost:8080/v1/embeddings",
            model="text-embedding-3-small",
            dimension=1536,
        )
        self.assertIsNotNone(he)

    def test_f3_empty_model_raises_value_error(self):
        with self.assertRaises(ValueError) as ctx:
            pyqql.HttpEmbedder(
                endpoint="http://localhost:8080/v1/embeddings",
                model="",
                dimension=768,
            )
        self.assertIn("model", str(ctx.exception).lower())

    def test_f4_missing_dimension_raises_type_error(self):
        with self.assertRaises(TypeError):
            pyqql.HttpEmbedder(
                endpoint="http://localhost:8080/v1/embeddings",
                model="test-model",
                dimension=None,
            )

    def test_f5_client_with_httpembedder_explain(self):
        he = pyqql.HttpEmbedder(
            endpoint="http://localhost:8080/v1/embeddings",
            model="test-model",
            dimension=768,
        )
        client = pyqql.Client(url="http://localhost:6333", embedder=he)
        result = client.explain('QUERY "hello" FROM docs LIMIT 5')
        self.assertTrue(result["ok"])

    def test_f6_client_with_dict_embedder_explain(self):
        embedder_dict = {
            "endpoint": "http://localhost:8080/v1/embeddings",
            "model": "text-embedding-3-small",
            "dimension": 1536,
        }
        client = pyqql.Client(url="http://localhost:6333", embedder=embedder_dict)
        result = client.explain('QUERY "hello" FROM docs LIMIT 5')
        self.assertTrue(result["ok"])

    def test_f7_dict_embedder_with_rerank_fields_accepted(self):
        """RT-05: remote embedder config with rerank_* fields must not error."""
        embedder_dict = {
            "endpoint": "http://localhost:8080/v1/embeddings",
            "model": "text-embedding-3-small",
            "dimension": 1536,
            "rerank_endpoint": "http://localhost:8080/rerank",
            "rerank_api_key": "rk-test-key",
            "rerank_model": "test-reranker",
        }
        client = pyqql.Client(url="http://localhost:6333", embedder=embedder_dict)
        result = client.explain('QUERY "hello" FROM docs LIMIT 5')
        self.assertTrue(result["ok"])

    def test_f8_dict_embedder_rerank_multi_image_all_together(self):
        """RT-05: full remote embedder config with all optional fields accepted."""
        embedder_dict = {
            "endpoint": "http://localhost:8080/v1/embeddings",
            "model": "text-embedding-3-small",
            "dimension": 1536,
            "api_key": "emb-key",
            "multi_endpoint": "http://localhost:8080/v1/multi",
            "multi_api_key": "multi-key",
            "multi_model": "colbert-model",
            "multi_dimension": 96,
            "image_endpoint": "http://localhost:8080/v1/images",
            "image_api_key": "img-key",
            "image_model": "clip-model",
            "image_dimension": 512,
            "rerank_endpoint": "http://localhost:8080/rerank",
            "rerank_api_key": "rk-key",
            "rerank_model": "bge-reranker",
        }
        client = pyqql.Client(url="http://localhost:6333", embedder=embedder_dict)
        self.assertIsNotNone(client)
        result = client.explain('QUERY "hello" FROM docs LIMIT 5')
        self.assertTrue(result["ok"])


# ============================================================================
# Category G: Full E2E pipeline against live Qdrant
# ============================================================================
# Category G: Full E2E pipeline against live Qdrant
# ============================================================================

@unittest.skipUnless(_qdrant_ok, "Qdrant not available at localhost:6333")
class TestE2EPipeline(unittest.TestCase):
    """G: End-to-end pipeline against live Qdrant."""

    @classmethod
    def setUpClass(cls):
        cls.client = pyqql.Client(url="http://localhost:6333")
        try:
            cls.client.execute(f"CREATE COLLECTION {E2E_COLLECTION}")
        except Exception:
            pass

    @classmethod
    def tearDownClass(cls):
        try:
            cls.client.execute(f"DROP COLLECTION {E2E_COLLECTION}")
        except Exception:
            pass

    def test_g1_show_collections(self):
        """SHOW COLLECTIONS returns known collections."""
        result = self.client.execute("SHOW COLLECTIONS")
        self.assertTrue(result["ok"])
        r0 = result["results"][0]
        collections = r0["data"]["result"]["collections"]
        names = [c["name"] for c in collections]
        self.assertIsInstance(names, list)

    def test_g2_count_collection(self):
        """COUNT on collection returns positive or zero count."""
        result = self.client.execute(f"COUNT FROM {E2E_COLLECTION}")
        self.assertTrue(result["ok"])
        count = result["results"][0]["data"]["result"]["count"]
        self.assertIsInstance(count, int)
        self.assertGreaterEqual(count, 0)

    def test_g3_count_with_filter(self):
        """COUNT with WHERE filter on data."""
        result = self.client.execute(
            f'COUNT FROM {E2E_COLLECTION} WHERE symbol = "AAPL"'
        )
        self.assertTrue(result["ok"])
        count = result["results"][0]["data"]["result"]["count"]
        self.assertIsInstance(count, int)

    def test_g4_count_with_comparison_filter(self):
        """COUNT with numeric filter."""
        result = self.client.execute(
            f"COUNT FROM {E2E_COLLECTION} WHERE volume > 10000000"
        )
        self.assertTrue(result["ok"])
        count = result["results"][0]["data"]["result"]["count"]
        self.assertIsInstance(count, int)

    def test_g5_create_and_drop_collection(self):
        """CREATE and DROP a collection end-to-end."""
        tmp_coll = f"tmp_{E2E_COLLECTION}"
        try:
            self.client.execute(f"DROP COLLECTION {tmp_coll}")
        except Exception:
            pass
        # Create
        r1 = self.client.execute(f"CREATE COLLECTION {tmp_coll}")
        self.assertTrue(r1["ok"])

        # Verify it appears
        r2 = self.client.execute("SHOW COLLECTIONS")
        names = [
            c["name"]
            for c in r2["results"][0]["data"]["result"]["collections"]
        ]
        self.assertIn(tmp_coll, names)

        # Drop
        r3 = self.client.execute(f"DROP COLLECTION {tmp_coll}")
        self.assertTrue(r3["ok"])

        # Verify gone
        r4 = self.client.execute("SHOW COLLECTIONS")
        names2 = [
            c["name"]
            for c in r4["results"][0]["data"]["result"]["collections"]
        ]
        self.assertNotIn(tmp_coll, names2)

    def test_g6_upsert_compiles_correct_route(self):
        """UPSERT compile_query produces correct Qdrant route structure."""
        cq = pyqql.compile_query(
            'UPSERT INTO test VALUES {"id":"x","vector":[1.0,2.0],"key":"val"}'
        )
        self.assertEqual(cq["method"], "PUT")
        self.assertIn("/collections/test/points", cq["path"])
        self.assertIn("points", cq["payload"])
        pt = cq["payload"]["points"][0]
        self.assertEqual(pt["payload"]["key"], "val")

    def test_g7_delete_compiles_correct_route(self):
        """DELETE compile_query produces correct Qdrant route structure."""
        cq = pyqql.compile_query('DELETE FROM test WHERE id = "doc-1"')
        self.assertIn("method", cq)
        self.assertIn("path", cq)


# ============================================================================
# Category H: Script-level execute
# ============================================================================

@unittest.skipUnless(_qdrant_ok, "Qdrant not available at localhost:6333")
class TestScriptExecute(unittest.TestCase):
    """H: Test multi-statement execution via Client.execute()."""

    @classmethod
    def setUpClass(cls):
        cls.client = pyqql.Client(url="http://localhost:6333")
        try:
            cls.client.execute(f"CREATE COLLECTION {E2E_COLLECTION}")
        except Exception:
            pass

    @classmethod
    def tearDownClass(cls):
        try:
            cls.client.execute(f"DROP COLLECTION {E2E_COLLECTION}")
        except Exception:
            pass

    def test_h1_execute_multi_string_script(self):
        result = self.client.execute(
            f"COUNT FROM {E2E_COLLECTION}; SHOW COLLECTIONS"
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failed"], 0)
        self.assertEqual(result["succeeded"], 2)
        self.assertEqual(len(result["results"]), 2)
        for r in result["results"]:
            self.assertTrue(r["ok"])

    def test_h2_execute_list_of_strings(self):
        result = self.client.execute(
            [f"COUNT FROM {E2E_COLLECTION}", "SHOW COLLECTIONS"]
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["succeeded"], 2)
        self.assertEqual(len(result["results"]), 2)

    def test_h3_execute_list_of_stmt_objects(self):
        stmts = pyqql.parse(f"COUNT FROM {E2E_COLLECTION}; SHOW COLLECTIONS")
        self.assertEqual(len(stmts), 2)
        result = self.client.execute(stmts)
        self.assertTrue(result["ok"])
        self.assertEqual(result["succeeded"], 2)

    def test_h4_execute_mixed_on_error_continue(self):
        """One valid, one invalid — on_error=continue returns report."""
        result = self.client.execute(
            [f"COUNT FROM {E2E_COLLECTION}", "INVALID !!! QQL SYNTAX"],
            on_error="continue",
        )
        self.assertFalse(result["ok"])
        self.assertEqual(result["failed"], 1)
        self.assertEqual(result["succeeded"], 1)
        self.assertEqual(len(result["results"]), 2)


# ============================================================================
# Category I: compile_query route contract
# ============================================================================

class TestCompileQueryRoute(unittest.TestCase):
    """I: Verify compile_query produces correct route dicts."""

    def test_i1_query_route_structure(self):
        cq = pyqql.compile_query('QUERY "test" FROM docs LIMIT 10')
        self.assertIsInstance(cq, dict)
        self.assertIn("method", cq)
        self.assertIn("path", cq)
        self.assertIn("payload", cq)

    def test_i2_route_method_is_post(self):
        cq = pyqql.compile_query('QUERY "test" FROM docs LIMIT 10')
        self.assertEqual(cq["method"], "POST")

    def test_i3_route_path_contains_collection(self):
        cq = pyqql.compile_query('QUERY "test" FROM mycoll LIMIT 10')
        self.assertIn("mycoll", cq["path"])

    def test_i4_route_payload_has_limit(self):
        cq = pyqql.compile_query('QUERY "test" FROM docs LIMIT 10')
        self.assertIn("limit", cq["payload"])
        self.assertEqual(cq["payload"]["limit"], 10)

    def test_i5_count_compiles(self):
        cq = pyqql.compile_query("COUNT FROM docs")
        self.assertIsInstance(cq, dict)
        self.assertIn("method", cq)
        self.assertIn("path", cq)

    def test_i6_upsert_compiles_with_correct_method(self):
        cq = pyqql.compile_query(
            'UPSERT INTO test VALUES {"id":"x","vector":[1.0,2.0],"k":"v"}'
        )
        self.assertIsInstance(cq, dict)
        self.assertEqual(cq["method"], "PUT")

    def test_i7_delete_compiles_with_correct_method(self):
        cq = pyqql.compile_query('DELETE FROM test WHERE id = "doc-1"')
        self.assertIsInstance(cq, dict)
        self.assertIn("method", cq)

    def test_i8_create_collection_compiles(self):
        cq = pyqql.compile_query("CREATE COLLECTION mytest")
        self.assertIsInstance(cq, dict)
        self.assertEqual(cq["method"], "PUT")
        self.assertIn("/collections/mytest", cq["path"])

    def test_i9_drop_collection_compiles(self):
        cq = pyqql.compile_query("DROP COLLECTION mytest")
        self.assertIsInstance(cq, dict)
        self.assertEqual(cq["method"], "DELETE")
        self.assertIn("/collections/mytest", cq["path"])

    def test_i10_show_collections_compiles(self):
        cq = pyqql.compile_query("SHOW COLLECTIONS")
        self.assertIsInstance(cq, dict)
        self.assertEqual(cq["method"], "GET")
        self.assertEqual(cq["path"], "/collections")


# ============================================================================
# Category J: Edge cases
# ============================================================================

class TestEdgeCases(unittest.TestCase):
    """J: Edge cases, corner conditions, and boundary behavior."""

    def test_j1_empty_string_parse_returns_empty(self):
        stmts = pyqql.parse("")
        self.assertIsInstance(stmts, list)
        self.assertEqual(len(stmts), 0)

    def test_j2_whitespace_only_parse_returns_empty(self):
        stmts = pyqql.parse("   \t\n   ")
        self.assertIsInstance(stmts, list)
        self.assertEqual(len(stmts), 0)

    def test_j3_is_valid_empty_is_true(self):
        """PAPERCUT: is_valid on empty string returns True."""
        self.assertTrue(pyqql.is_valid(""))

    def test_j4_is_valid_whitespace_is_true(self):
        """PAPERCUT: is_valid on whitespace returns True."""
        self.assertTrue(pyqql.is_valid("   "))

    def test_j5_is_valid_sql_is_false(self):
        self.assertFalse(pyqql.is_valid("SELECT * FROM docs"))

    def test_j6_stmt_shard_key_getter_none(self):
        stmt = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')[0]
        self.assertIsNone(stmt.shard_key)

    def test_j7_stmt_shard_key_setter_string(self):
        stmt = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')[0]
        stmt.shard_key = "us-east-1"
        self.assertEqual(stmt.shard_key, "us-east-1")

    def test_j8_stmt_shard_key_setter_none(self):
        stmt = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')[0]
        stmt.shard_key = "europe"
        stmt.shard_key = None
        self.assertIsNone(stmt.shard_key)

    def test_j9_count_shard_key(self):
        stmt = pyqql.parse("COUNT FROM docs")[0]
        self.assertIsNone(stmt.shard_key)
        stmt.shard_key = "asia"
        self.assertEqual(stmt.shard_key, "asia")

    def test_j10_delete_payload_shard_key(self):
        stmt = pyqql.parse(
            "DELETE PAYLOAD draft FROM docs WHERE status = 'archived'"
        )[0]
        stmt.shard_key = "tenant-a"
        self.assertEqual(stmt.shard_key, "tenant-a")
        self.assertEqual(
            stmt.to_dict()["DeletePayload"]["shard_key"],
            "tenant-a",
        )

    def test_j11_stmt_shard_key_property(self):
        """SHARD in QQL + Stmt.shard_key property (no inject_shard_key)."""
        stmts = pyqql.parse("QUERY TEXT 'x' FROM docs SHARD 'honeywell' LIMIT 5")
        assert stmts[0].shard_key == "honeywell"
        stmts2 = pyqql.parse("QUERY TEXT 'x' FROM docs LIMIT 5")
        stmts2[0].shard_key = "acme"
        assert stmts2[0].shard_key == "acme"
        # empty clears
        stmts2[0].shard_key = ""
        assert stmts2[0].shard_key is None


    def test_j12_show_collections_to_json(self):
        stmt = pyqql.parse("SHOW COLLECTIONS")[0]
        j = stmt.to_json()
        self.assertIsInstance(j, str)
        self.assertEqual(j, '{"ShowCollections":{}}')

    def test_j13_module_level_explain(self):
        result = pyqql.explain('QUERY "test" FROM docs LIMIT 5')
        self.assertIsInstance(result, dict)
        self.assertTrue(result["ok"])

    def test_j14_parse_duplicate_statements(self):
        stmts = pyqql.parse("COUNT FROM docs; COUNT FROM docs")
        self.assertEqual(len(stmts), 2)

    def test_j15_to_dict_stable(self):
        stmt = pyqql.parse('QUERY "hello" FROM docs LIMIT 5')[0]
        d1 = stmt.to_dict()
        d2 = stmt.to_dict()
        self.assertEqual(d1, d2)

    def test_j14_is_valid_show_collections(self):
        self.assertTrue(pyqql.is_valid("SHOW COLLECTIONS"))

    def test_j15_tokenize_empty(self):
        tokens = pyqql.tokenize("")
        self.assertIsInstance(tokens, list)
        self.assertEqual(len(tokens), 0)

    def test_j16_tokenize_whitespace(self):
        tokens = pyqql.tokenize("   ")
        self.assertIsInstance(tokens, list)
        self.assertEqual(len(tokens), 0)

    def test_j17_delete_to_dict(self):
        stmts = pyqql.parse('DELETE FROM test WHERE id = "doc-1"')
        d = stmts[0].to_dict()
        self.assertIn("Delete", d)
        self.assertEqual(d["Delete"]["collection"], "test")
        self.assertEqual(d["Delete"]["selector"]["Id"]["String"], "doc-1")

    def test_j18_upsert_to_dict(self):
        stmts = pyqql.parse(
            'UPSERT INTO test VALUES {"id":"x","vector":[1.0,2.0],"k":"v"}'
        )
        d = stmts[0].to_dict()
        self.assertIn("Upsert", d)
        self.assertEqual(d["Upsert"]["collection"], "test")
        points = d["Upsert"]["points"]
        self.assertEqual(len(points), 1)
        self.assertEqual(points[0]["id"]["String"], "x")

    def test_j19_upsert_payload_fields_are_top_level(self):
        """Payload fields in UPSERT JSON go directly under point payload."""
        cq = pyqql.compile_query(
            'UPSERT INTO test VALUES {"id":"x","vector":[1.0],"myfield":"myval","count":42}'
        )
        pt = cq["payload"]["points"][0]
        self.assertEqual(pt["payload"]["myfield"], "myval")
        self.assertEqual(pt["payload"]["count"], 42)

    def test_j20_upsert_payload_key_is_nested(self):
        """A key literally named 'payload' in VALUES becomes nested."""
        cq = pyqql.compile_query(
            'UPSERT INTO test VALUES {"id":"x","vector":[1.0],"payload":{"nested":"yes"}}'
        )
        pt = cq["payload"]["points"][0]
        self.assertIn("payload", pt["payload"])
        self.assertEqual(pt["payload"]["payload"]["nested"], "yes")

    def test_j21_upsert_no_vector_compiles(self):
        """UPSERT without vector field still compiles."""
        cq = pyqql.compile_query(
            'UPSERT INTO test VALUES {"id":"x","text":"just metadata"}'
        )
        self.assertIn("points", cq["payload"])
        self.assertNotIn("vector", cq["payload"]["points"][0])

    def test_j22_numeric_id_upsert(self):
        """UPSERT with numeric id compiles."""
        cq = pyqql.compile_query(
            'UPSERT INTO test VALUES {"id":42,"vector":[0.1,0.2]}'
        )
        pt = cq["payload"]["points"][0]
        self.assertEqual(pt["id"], 42)

    def test_j23_create_collection_to_dict(self):
        """CREATE COLLECTION to_dict returns CreateCollection."""
        stmts = pyqql.parse("CREATE COLLECTION mytest")
        d = stmts[0].to_dict()
        self.assertIsInstance(d, dict)
        self.assertIn("CreateCollection", d)
        self.assertEqual(d["CreateCollection"]["collection"], "mytest")

    def test_j24_delete_payload_compile(self):
        """DELETE PAYLOAD statement compiles to /points/payload/delete route."""
        cq = pyqql.compile_query(
            "DELETE PAYLOAD draft, temp_token FROM docs WHERE status = 'archived' SHARD 'tenant_1'"
        )
        self.assertEqual(cq["method"], "POST")
        self.assertEqual(cq["path"], "/collections/docs/points/payload/delete")
        self.assertEqual(cq["payload"]["keys"], ["draft", "temp_token"])

    def test_j25_count_exact_compile(self):
        """COUNT statement with exact = true compiles exact flag."""
        cq = pyqql.compile_query(
            "COUNT FROM docs WHERE active = true WITH (exact = true)"
        )
        self.assertEqual(cq["method"], "POST")
        self.assertTrue(cq["payload"]["exact"])

    def test_j26_group_by_offset_compile(self):
        """GROUP BY statement with OFFSET computes effective limit (limit + offset)."""
        cq = pyqql.compile_query(
            "QUERY TEXT 'search' FROM docs GROUP BY category LIMIT 10 OFFSET 5"
        )
        self.assertEqual(cq["method"], "POST")
        self.assertIn("/query/groups", cq["path"])
        self.assertEqual(cq["payload"]["limit"], 15)


# ============================================================================
# Category K: Parameter Binding
# ============================================================================

class TestParameterBinding(unittest.TestCase):
    """K: Test parameter binding (:name and ?)."""

    def test_k1_bind_named_dict(self):
        q = "QUERY 'shoes' FROM products WHERE category = :cat AND price < :max_p"
        res = pyqql.bind(q, {"cat": "sneakers", "max_p": 100})
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND price < 100",
        )

    def test_k2_bind_positional_list(self):
        q = "QUERY 'shoes' FROM products WHERE category = ? AND in_stock = ?"
        res = pyqql.bind(q, ["sneakers", True])
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND in_stock = true",
        )

    def test_k3_bind_named_again(self):
        q = "QUERY 'shoes' FROM products WHERE category = :cat"
        res = pyqql.bind(q, {"cat": "boots"})
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'boots'",
        )

    def test_k4_bind_positional_again(self):
        q = "QUERY 'shoes' FROM products WHERE category = ?"
        res = pyqql.bind(q, ["boots"])
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE category = 'boots'",
        )

    def test_k5_bind_preserves_dollar_identifiers(self):
        q = "QUERY 'shoes' FROM products WHERE $category = :cat AND $1 = 42"
        res = pyqql.bind(q, {"cat": "boots"})
        self.assertEqual(
            res,
            "QUERY 'shoes' FROM products WHERE $category = 'boots' AND $1 = 42",
        )


# ============================================================================
# Main
# ============================================================================

if __name__ == "__main__":
    unittest.main(verbosity=2)

