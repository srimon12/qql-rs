"""Embedder parity: PyHttpEmbedder multi/image/rerank groups (offline, no network).

Covers:
- dense validation preserved (endpoint/model/dimension required)
- new kwargs default None, accept full groups, allow 0 dims
- empty-string endpoints treated as unset (no error)
- class+dict equivalence via offline Client.explain
"""

import unittest

import pyqql


class TestHttpEmbedderParity(unittest.TestCase):
    def test_positional_backward_compat(self):
        he = pyqql.HttpEmbedder("http://localhost:11434/v1/embeddings", "m", 8)
        client = pyqql.Client("http://localhost:6333", embedder=he)
        self.assertTrue(client.explain("QUERY 'hi' FROM docs LIMIT 1")["ok"])

    def test_dense_validation_preserved(self):
        with self.assertRaisesRegex(ValueError, "endpoint is required"):
            pyqql.HttpEmbedder(endpoint="", model="m", dimension=8)
        with self.assertRaisesRegex(ValueError, "model is required"):
            pyqql.HttpEmbedder(endpoint="http://x", model="  ", dimension=8)
        with self.assertRaisesRegex(ValueError, "dimension must be positive"):
            pyqql.HttpEmbedder(endpoint="http://x", model="m", dimension=0)

    def test_full_groups_accepted(self):
        he = pyqql.HttpEmbedder(
            endpoint="http://localhost:11434/v1/embeddings",
            model="dense-m",
            dimension=8,
            api_key="k",
            multi_endpoint="http://localhost:11434/v1/multi",
            multi_api_key="mk",
            multi_model="colbert",
            multi_dimension=4,
            image_endpoint="http://localhost:11434/v1/image",
            image_api_key="ik",
            image_model="clip",
            image_dimension=8,
            rerank_endpoint="http://localhost:11434/rerank",
            rerank_api_key="rk",
            rerank_model="bge",
        )
        client = pyqql.Client("http://localhost:6333", embedder=he)
        self.assertTrue(client.explain("QUERY 'hi' FROM docs LIMIT 1")["ok"])

    def test_zero_dims_allowed_and_empty_strings_unset(self):
        # 0 skips checks (multi) / falls back to dense dim (image) in core;
        # empty-string endpoints treated as unset.
        he = pyqql.HttpEmbedder(
            endpoint="http://localhost:11434/v1/embeddings",
            model="dense-m",
            dimension=8,
            multi_endpoint="",
            multi_model="",
            multi_dimension=0,
            image_endpoint="   ",
            image_model="",
            image_dimension=0,
            rerank_endpoint="",
            rerank_model="",
        )
        client = pyqql.Client("http://localhost:6333", embedder=he)
        self.assertTrue(client.explain("QUERY 'hi' FROM docs LIMIT 1")["ok"])

    def test_class_dict_equivalence(self):
        kwargs = dict(
            endpoint="http://localhost:11434/v1/embeddings",
            model="dense-m",
            dimension=8,
            api_key="k",
            multi_endpoint="http://localhost:11434/v1/multi",
            multi_api_key="mk",
            multi_model="colbert",
            multi_dimension=4,
            image_endpoint="http://localhost:11434/v1/image",
            image_api_key="ik",
            image_model="clip",
            image_dimension=8,
            rerank_endpoint="http://localhost:11434/rerank",
            rerank_api_key="rk",
            rerank_model="bge",
        )
        he = pyqql.HttpEmbedder(**kwargs)
        dict_emb = {
            "endpoint": kwargs["endpoint"],
            "model": kwargs["model"],
            "dimension": kwargs["dimension"],
            "api_key": kwargs["api_key"],
            "multi_endpoint": kwargs["multi_endpoint"],
            "multi_api_key": kwargs["multi_api_key"],
            "multi_model": kwargs["multi_model"],
            "multi_dimension": kwargs["multi_dimension"],
            "image_endpoint": kwargs["image_endpoint"],
            "image_api_key": kwargs["image_api_key"],
            "image_model": kwargs["image_model"],
            "image_dimension": kwargs["image_dimension"],
            "rerank_endpoint": kwargs["rerank_endpoint"],
            "rerank_api_key": kwargs["rerank_api_key"],
            "rerank_model": kwargs["rerank_model"],
        }
        c1 = pyqql.Client("http://localhost:6333", embedder=he)
        c2 = pyqql.Client("http://localhost:6333", embedder=dict_emb)
        r1 = c1.explain("QUERY 'hi' FROM docs LIMIT 1")
        r2 = c2.explain("QUERY 'hi' FROM docs LIMIT 1")
        self.assertTrue(r1["ok"])
        self.assertEqual(r1, r2)

    def test_dict_none_and_empty_tolerant(self):
        dict_emb = {
            "endpoint": "http://localhost:11434/v1/embeddings",
            "model": "dense-m",
            "dimension": 8,
            "multi_endpoint": None,
            "multi_api_key": None,
            "multi_model": None,
            "multi_dimension": None,
            "image_endpoint": "",
            "image_api_key": None,
            "image_model": "",
            "image_dimension": None,
            "rerank_endpoint": "",
            "rerank_api_key": None,
            "rerank_model": None,
        }
        client = pyqql.Client("http://localhost:6333", embedder=dict_emb)
        self.assertTrue(client.explain("QUERY 'hi' FROM docs LIMIT 1")["ok"])


if __name__ == "__main__":
    unittest.main()
