"""pyqql-edge tests — parser + edge executor + red-team constraints."""

from __future__ import annotations

import os
import tempfile
import unittest
import uuid

import pyqql_edge


def _find_cache() -> str | None:
    candidates = [
        os.path.join(os.path.dirname(__file__), "..", "..", "..", ".fastembed_cache"),
        os.path.join(os.path.dirname(__file__), "..", ".fastembed_cache"),
        os.environ.get("FASTEMBED_CACHE_DIR"),
        os.path.join(os.environ["HF_HOME"], "hub") if os.environ.get("HF_HOME") else None,
    ]
    for d in candidates:
        if d and os.path.isdir(os.path.join(d, "models--Xenova--bge-small-en-v1.5")):
            return d
    return None


CACHE = _find_cache()


def _uuid(n: int) -> str:
    return f"550e8400-e29b-41d4-a716-44665544{n:04d}"


def _local(tmpdir: str, **kwargs):
    opts = {"on_disk_payload": False, **kwargs}
    if CACHE and "cache_dir" not in opts:
        opts["cache_dir"] = CACHE
    return pyqql_edge.local_executor(tmpdir, **opts)


class _EdgeCase:
    """Temp data dir that keeps the Client alive until after assertions, then
    drops the client *before* the directory is deleted.

    qdrant-edge panics if the data dir is removed while a background flush is
    still running — a real lifecycle footgun. Tests must not race that.
    """

    def __enter__(self):
        self._td = tempfile.TemporaryDirectory()
        self.dir = self._td.name
        self.exec = _local(self.dir)
        return self

    def __exit__(self, *exc):
        # Explicitly flush and release shards before removing the directory.
        self.exec.close()
        self.exec = None
        self._td.cleanup()
        return False


class TestParser(unittest.TestCase):
    def test_parse(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        stmts = pyqql_edge.parse(query)
        self.assertIsInstance(stmts, list)
        self.assertEqual(len(stmts), 1)
        stmt = stmts[0]
        self.assertTrue(hasattr(stmt, "to_dict"))
        d = stmt.to_dict()
        self.assertIn("Query", d)
        self.assertEqual(d["Query"]["collection"]["Explicit"], "docs")
        self.assertEqual(
            d["Query"]["expression"]["Nearest"]["input"]["Text"]["text"], "hello"
        )

    def test_explain(self):
        plan = pyqql_edge.explain("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIn("Statement: QUERY", plan)
        self.assertIn("Collection: docs", plan)

    def test_parse_script(self):
        results = pyqql_edge.parse(
            "QUERY 'test' FROM users LIMIT 5; CREATE COLLECTION items"
        )
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
        self.assertFalse(pyqql_edge.is_valid("SELECT * FROM docs"))

    def test_compile(self):
        result = pyqql_edge.compile_query("QUERY 'hello' FROM docs LIMIT 10")
        self.assertIsInstance(result, dict)
        self.assertEqual(result["method"], "POST")

    def test_invalid(self):
        with self.assertRaises(SyntaxError):
            pyqql_edge.parse("invalid syntax")

    def test_invalid_filter_operator(self):
        stmt = pyqql_edge.parse("QUERY 'hello' FROM docs LIMIT 10")[0]
        with self.assertRaisesRegex(SyntaxError, "unsupported comparison operator"):
            stmt.inject_filter("tenant_id", "contains", "acme")
        with self.assertRaisesRegex(SyntaxError, "unsupported comparison operator"):
            pyqql_edge.inject_filter(
                "QUERY 'hello' FROM docs LIMIT 10",
                "tenant_id",
                "contains",
                "acme",
            )


class TestModelSelection(unittest.TestCase):
    def test_list_embedding_models(self):
        models = pyqql_edge.list_embedding_models()
        self.assertIsInstance(models, list)
        self.assertGreater(len(models), 5)
        names = {m["name"] for m in models}
        self.assertIn("BGESmallENV15", names)
        bge = next(m for m in models if m["name"] == "BGESmallENV15")
        self.assertEqual(bge["dim"], 384)
        self.assertIn("bge-small", bge["model_code"])

    def test_invalid_model_rejected(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaisesRegex(RuntimeError, "unknown embedding model"):
                pyqql_edge.local_executor(
                    tmpdir, on_disk_payload=False, model="not-a-real-model-xyz"
                )

    def test_model_short_alias(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            exec_ = _local(tmpdir, model="bge-small-en-v1.5")
            self.assertIsInstance(exec_, pyqql_edge.Client)


class TestEdgeExecutor(unittest.TestCase):
    def test_local_executor_basics(self):
        with _EdgeCase() as edge:
            self.assertIsInstance(edge.exec, pyqql_edge.Client)
            plan = edge.exec.explain("QUERY 'hello' FROM docs LIMIT 10")
            self.assertIn("Statement: QUERY", plan)
            with self.assertRaises(ValueError):
                edge.exec.execute("SHOW COLLECTIONS", on_error="typo")
            report = edge.exec.execute("invalid syntax", on_error="continue")
            self.assertFalse(report["ok"])
            self.assertEqual(report["failed"], 1)
            self.assertEqual(report["results"][0]["operation"], "PARSE")

    def test_e2e_hybrid_pipeline(self):
        with _EdgeCase() as edge:
            r = edge.exec.execute("CREATE COLLECTION py_test HYBRID")
            self.assertTrue(r["ok"], r)

            id1, id2 = _uuid(1), _uuid(2)
            r = edge.exec.execute(
                f'UPSERT INTO py_test VALUES '
                f'{{id: "{id1}", text: "Rust is a systems programming language that runs blazingly fast", created_at: 1}}, '
                f'{{id: "{id2}", text: "Python is great for data science and machine learning", created_at: 2}}'
            )
            self.assertTrue(r["ok"], r)

            r = edge.exec.execute(
                "QUERY 'fast programming language' FROM py_test USING dense LIMIT 2"
            )
            self.assertTrue(r["ok"], r)
            hits = r["results"][0]["data"]
            self.assertIsInstance(hits, list)
            self.assertGreater(len(hits), 0)

            r = edge.exec.execute("COUNT FROM py_test")
            self.assertTrue(r["ok"], r)
            count = r["results"][0]["data"]["result"]["count"]
            self.assertEqual(count, 2)

            # numeric ids work
            r = edge.exec.execute(
                'UPSERT INTO py_test VALUES {id: 7, text: "numeric id works"}'
            )
            self.assertTrue(r["ok"], r)

            r = edge.exec.execute(f'DELETE FROM py_test WHERE id = "{id2}"')
            self.assertTrue(r["ok"], r)

            r = edge.exec.execute("COUNT FROM py_test")
            count = r["results"][0]["data"]["result"]["count"]
            self.assertEqual(count, 2)  # id1 + numeric 7

    def test_native_query_variants(self):
        with _EdgeCase() as edge:
            self.assertTrue(edge.exec.execute("CREATE COLLECTION variants HYBRID")["ok"])
            self.assertTrue(
                edge.exec.execute(
                    f'UPSERT INTO variants VALUES {{id: "{_uuid(1)}", text: "hello", created_at: 1}}'
                )["ok"]
            )

            r = edge.exec.execute(
                "QUERY MMR TEXT 'hello' DIVERSITY 0.4 CANDIDATES 10 "
                "FROM variants USING dense LIMIT 1"
            )
            self.assertTrue(r["ok"], r)

            r = edge.exec.execute("QUERY SAMPLE RANDOM FROM variants LIMIT 1")
            self.assertTrue(r["ok"], r)

            r = edge.exec.execute(
                "CREATE INDEX ON COLLECTION variants FOR created_at TYPE integer"
            )
            self.assertTrue(r["ok"], r)
            r = edge.exec.execute("QUERY ORDER BY created_at DESC FROM variants LIMIT 1")
            self.assertTrue(r["ok"], r)

            r = edge.exec.execute(
                "WITH candidates AS (QUERY TEXT 'hello' USING dense LIMIT 10) "
                "QUERY FORMULA $score * 2 DEFAULTS (score = 0.0) FROM variants "
                "PREFETCH (candidates) LIMIT 1"
            )
            self.assertTrue(r["ok"], r)

            for query in (
                "QUERY CONTEXT (POSITIVE TEXT 'hello' NEGATIVE TEXT 'bad') "
                "FROM variants USING dense LIMIT 1",
                "QUERY DISCOVER TARGET TEXT 'hello' CONTEXT "
                "(POSITIVE TEXT 'hello' NEGATIVE TEXT 'bad') "
                "FROM variants USING dense LIMIT 1",
                "QUERY RELEVANCE FEEDBACK TARGET TEXT 'hello' FEEDBACK "
                "((TEXT 'hello', 0.8)) STRATEGY NAIVE (a = 1, b = 1, c = 1) "
                "FROM variants USING dense LIMIT 1",
            ):
                r = edge.exec.execute(query)
                self.assertTrue(r["ok"], r)

    def test_non_uuid_string_id_rejected(self):
        with _EdgeCase() as edge:
            edge.exec.execute("CREATE COLLECTION t HYBRID")
            r = edge.exec.execute(
                'UPSERT INTO t VALUES {id: "doc-not-a-uuid", text: "nope"}',
                on_error="continue",
            )
            self.assertFalse(r["ok"])
            msg = r["results"][0]["message"].lower()
            self.assertIn("uuid", msg)

    def test_query_without_using_on_hybrid(self):
        with _EdgeCase() as edge:
            edge.exec.execute("CREATE COLLECTION t HYBRID")
            edge.exec.execute(
                f'UPSERT INTO t VALUES {{id: "{_uuid(1)}", text: "hello world"}}'
            )
            r = edge.exec.execute(
                "QUERY 'hello' FROM t LIMIT 1", on_error="continue"
            )
            self.assertTrue(r["ok"], r)

    def test_group_by_unsupported(self):
        with _EdgeCase() as edge:
            edge.exec.execute("CREATE COLLECTION t HYBRID")
            r = edge.exec.execute(
                "QUERY 'x' FROM t USING dense GROUP BY cat LIMIT 5",
                on_error="continue",
            )
            self.assertFalse(r["ok"])

    def test_point_reference_recommendation_rejected(self):
        with _EdgeCase() as edge:
            edge.exec.execute("CREATE COLLECTION t HYBRID")
            r = edge.exec.execute(
                "QUERY RECOMMEND POSITIVE (1) STRATEGY best_score "
                "FROM t USING dense LIMIT 1",
                on_error="continue",
            )
            self.assertFalse(r["ok"])
            self.assertIn("point-reference", r["results"][0]["message"])

    def test_model_mismatch_rejected(self):
        with _EdgeCase() as edge:
            edge.exec.execute("CREATE COLLECTION t HYBRID")
            edge.exec.execute(
                f'UPSERT INTO t VALUES {{id: "{_uuid(1)}", text: "hello"}}'
            )
            r = edge.exec.execute(
                "QUERY 'x' FROM t USING dense MODEL 'definitely-not-loaded' LIMIT 1",
                on_error="continue",
            )
            self.assertFalse(r["ok"])

    def test_dimension_mismatch(self):
        with _EdgeCase() as edge:
            r = edge.exec.execute(
                "CREATE COLLECTION wrong (dense VECTOR(16, COSINE), sparse SPARSE)"
            )
            self.assertTrue(r["ok"], r)
            r = edge.exec.execute(
                f'UPSERT INTO wrong VALUES {{id: "{_uuid(1)}", text: "dim boom"}}',
                on_error="continue",
            )
            self.assertFalse(r["ok"])

    def test_show_collections(self):
        with _EdgeCase() as edge:
            edge.exec.execute("CREATE COLLECTION a HYBRID")
            r = edge.exec.execute("SHOW COLLECTIONS")
            self.assertTrue(r["ok"], r)


if __name__ == "__main__":
    unittest.main()
