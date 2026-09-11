"""pyqql-edge-only tests for the client-side BM25 document parameters.

Not part of the byte-identical ``test_dx.py`` pair: these cover the edge
executor constructors' ``bm25_k1`` / ``bm25_b`` / ``bm25_avg_len`` kwargs.

BM25 params are a client-side, **write-path-only** knob: they shape how
documents are encoded (tf saturation / length normalization), never query
weights or server-side inference. Vectors written before a change keep their
values; re-ingest to apply.
"""

import shutil
import tempfile
import unittest

import pyqql_edge


class TestBm25ParamsValidation(unittest.TestCase):
    """Invalid values fail closed with QQL-VALIDATION-CONFIG before any model loads."""

    def test_local_executor_rejects_invalid_bm25_params(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_bm25_bad_")
        try:
            cases = [
                {"bm25_k1": 0.0},
                {"bm25_k1": -1.0},
                {"bm25_k1": float("nan")},
                {"bm25_k1": float("inf")},
                {"bm25_b": -0.1},
                {"bm25_b": 1.5},
                {"bm25_b": float("nan")},
                {"bm25_avg_len": 0.0},
                {"bm25_avg_len": -5.0},
                {"bm25_avg_len": float("inf")},
            ]
            for kwargs in cases:
                with self.assertRaises(pyqql_edge.QqlValidationError) as ctx:
                    pyqql_edge.local_executor(
                        tmpdir, on_disk_payload=False, **kwargs
                    )
                self.assertEqual(
                    ctx.exception.code,
                    "QQL-VALIDATION-CONFIG",
                    f"{kwargs} must fail closed",
                )
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)

    def test_http_executor_rejects_invalid_bm25_params(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_bm25_http_bad_")
        try:
            with self.assertRaises(pyqql_edge.QqlValidationError) as ctx:
                pyqql_edge.http_executor(
                    tmpdir,
                    "http://127.0.0.1:9/v1/embeddings",
                    "",
                    "unused-dense",
                    3,
                    on_disk_payload=False,
                    bm25_b=2.0,
                )
            self.assertEqual(ctx.exception.code, "QQL-VALIDATION-CONFIG")
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)

    def test_one_shot_execute_rejects_invalid_bm25_params(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_bm25_once_bad_")
        try:
            with self.assertRaises(pyqql_edge.QqlValidationError) as ctx:
                pyqql_edge.execute(
                    "COUNT FROM nope",
                    data_dir=tmpdir,
                    on_disk_payload=False,
                    bm25_avg_len=0.0,
                )
            self.assertEqual(ctx.exception.code, "QQL-VALIDATION-CONFIG")
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)


class TestBm25ParamsEndToEnd(unittest.TestCase):
    """Configured params must land in the sparse vector qdrant-edge stores."""

    def test_configured_params_land_in_stored_sparse_vector(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_bm25_")
        try:
            client = pyqql_edge.local_executor(
                tmpdir,
                on_disk_payload=False,
                bm25_k1=2.0,
                bm25_b=0.5,
                bm25_avg_len=4.0,
            )
            self.assertTrue(
                client.execute("CREATE COLLECTION bm25_docs (sparse SPARSE)").ok
            )
            self.assertTrue(
                client.execute(
                    "UPSERT INTO bm25_docs VALUES {id: 1, text: 'cat sat mat cat'}"
                ).ok
            )

            report = client.execute(
                "QUERY POINTS (1) FROM bm25_docs WITH VECTOR true"
            )
            self.assertTrue(report.ok, report)
            hits = report.hits(0)
            self.assertEqual(len(hits), 1)
            sparse = hits[0].vector["sparse"]
            self.assertEqual(len(sparse["indices"]), 3)
            values = sorted(sparse["values"])
            # dl=4, avg_len=4, k1=2, b=0.5 → denom_scale=2:
            # tf(cat)=2 → 1.5; tf(sat)=tf(mat)=1 → 1.0.
            self.assertAlmostEqual(values[0], 1.0, places=6)
            self.assertAlmostEqual(values[1], 1.0, places=6)
            self.assertAlmostEqual(values[2], 1.5, places=6)
            client.close()
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)

    def test_unset_params_keep_qdrant_server_defaults(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_bm25_default_")
        try:
            client = pyqql_edge.local_executor(tmpdir, on_disk_payload=False)
            self.assertTrue(
                client.execute("CREATE COLLECTION default_docs (sparse SPARSE)").ok
            )
            self.assertTrue(
                client.execute(
                    "UPSERT INTO default_docs VALUES "
                    "{id: 1, text: 'Recipe for baking chocolate chip cookies'}"
                ).ok
            )
            report = client.execute(
                "QUERY POINTS (1) FROM default_docs WITH VECTOR true"
            )
            self.assertTrue(report.ok, report)
            sparse = report.hits(0)[0].vector["sparse"]
            # Golden server qdrant/bm25 defaults: 5 stems (stopword "for"
            # removed), tf=1, dl=5, k1=1.2, b=0.75, avg_len=256.
            self.assertEqual(len(sparse["indices"]), 5)
            for value in sparse["values"]:
                self.assertAlmostEqual(value, 1.6697302, places=6)
            client.close()
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
