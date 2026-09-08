"""Official Qdrant SDK (qdrant-client) scenario implementations.

One method per benchmark scenario. Kept in its own file so the LOC report
counts exactly the application code a Qdrant user writes — no harness,
timing, or parity logic is included.

Every method returns plain Python objects (hits, counts, dicts) so the
harness can compare results against the QQL side without knowing either
SDK's response shapes.
"""
from __future__ import annotations

import time

import numpy as np
from qdrant_client import QdrantClient
from qdrant_client import models as M

from config import BATCH_BERLIN, BATCH_LEGAL


class OfficialScenarios:
    """qdrant-client 1.19 — the reference Qdrant Python SDK."""

    def __init__(self, url: str):
        self.client = QdrantClient(url=url, timeout=120, check_compatibility=False)

    def close(self) -> None:
        self.client.close()

    # ------------------------------------------------------------ setup ----
    def drop_collection(self, name: str) -> None:
        self.client.delete_collection(name)

    def create_berlin(self, name: str) -> None:
        self.client.create_collection(
            collection_name=name,
            vectors_config={
                "dense": M.VectorParams(size=384, distance=M.Distance.COSINE),
            },
            sparse_vectors_config={
                # BM25 vectors need the idf modifier for true BM25 scoring —
                # the same default QQL applies to `sparse SPARSE` columns.
                "bm25": M.SparseVectorParams(modifier=M.Modifier.IDF)
            },
        )
        self.client.create_payload_index(name, "district", M.PayloadSchemaType.KEYWORD)
        self.client.create_payload_index(name, "price", M.PayloadSchemaType.FLOAT)

    def create_legal(self, name: str) -> None:
        self.client.create_collection(
            collection_name=name,
            vectors_config={
                "dense": M.VectorParams(size=384, distance=M.Distance.COSINE),
                "colbert": M.VectorParams(
                    size=128,
                    distance=M.Distance.COSINE,
                    multivector_config=M.MultiVectorConfig(
                        comparator=M.MultiVectorComparator.MAX_SIM
                    ),
                    hnsw_config=M.HnswConfigDiff(m=0),
                ),
            },
            sparse_vectors_config={
                # BM25 vectors need the idf modifier for true BM25 scoring —
                # the same default QQL applies to `sparse SPARSE` columns.
                "bm25": M.SparseVectorParams(modifier=M.Modifier.IDF)
            },
        )
        self.client.create_payload_index(name, "court", M.PayloadSchemaType.KEYWORD)
        self.client.create_payload_index(name, "year", M.PayloadSchemaType.INTEGER)

    # ----------------------------------------------------------- ingest ----
    def _points(self, docs, dense, sparse, colbert=None):
        points = []
        for i, doc in enumerate(docs):
            vector = {
                "dense": dense[i].tolist(),
                "bm25": M.SparseVector(indices=sparse[i]["indices"], values=sparse[i]["values"]),
            }
            if colbert is not None:
                vector["colbert"] = colbert[i]
            # `id` addresses the point; QQL (and Qdrant convention) keeps it
            # out of the payload.
            payload = {k: v for k, v in doc.items() if k != "id"}
            points.append(M.PointStruct(id=doc["id"], vector=vector, payload=payload))
        return points

    def ingest_berlin(self, name: str, docs, dense, sparse) -> float:
        points = self._points(docs, dense, sparse)
        t0 = time.perf_counter()
        for i in range(0, len(points), BATCH_BERLIN):
            self.client.upsert(name, points[i:i + BATCH_BERLIN], wait=False)
        return time.perf_counter() - t0

    def ingest_legal(self, name: str, docs, dense, sparse, colbert) -> float:
        points = self._points(docs, dense, sparse, colbert)
        t0 = time.perf_counter()
        for i in range(0, len(points), BATCH_LEGAL):
            self.client.upsert(name, points[i:i + BATCH_LEGAL], wait=False)
        return time.perf_counter() - t0

    # ------------------------------------------------------------- reads ----
    def query_dense(self, name: str, qvec: list[float]) -> list:
        return self.client.query_points(
            name, query=qvec, using="dense", limit=10, with_payload=True,
        ).points

    def query_dense_filtered(self, name: str, qvec: list[float]) -> list:
        flt = M.Filter(must=[
            M.FieldCondition(key="price", range=M.Range(lt=150.0)),
            M.FieldCondition(key="guests", range=M.Range(gte=2)),
        ])
        return self.client.query_points(
            name, query=qvec, using="dense", limit=10, query_filter=flt, with_payload=True,
        ).points

    def query_sparse(self, name: str, svec: dict) -> list:
        return self.client.query_points(
            name,
            query=M.SparseVector(indices=svec["indices"], values=svec["values"]),
            using="bm25", limit=10, with_payload=True,
        ).points

    def query_hybrid(self, name: str, qvec: list[float], svec: dict) -> list:
        # hnsw_ef=128 on the dense leg: fused rankings must be deterministic
        # across the two independently built collections.
        return self.client.query_points(
            name,
            prefetch=[
                M.Prefetch(query=qvec, using="dense", limit=50,
                           params=M.SearchParams(hnsw_ef=128, exact=False)),
                M.Prefetch(
                    query=M.SparseVector(indices=svec["indices"], values=svec["values"]),
                    using="bm25", limit=50,
                ),
            ],
            query=M.FusionQuery(fusion=M.Fusion.RRF),
            limit=10, with_payload=True,
        ).points

    def query_colbert(self, name: str, mvec: list) -> list:
        return self.client.query_points(
            name, query=mvec, using="colbert", limit=10, with_payload=True,
        ).points

    def scroll_pages(self, name: str, pages: int, batch: int) -> list[int]:
        ids, offset = [], None
        for _ in range(pages):
            points, offset = self.client.scroll(
                name, limit=batch, offset=offset, with_payload=True,
            )
            ids.extend(p.id for p in points)
            if offset is None:
                break
        return ids

    def count_berlin(self, name: str) -> int:
        flt = M.Filter(must=[M.FieldCondition(key="price", range=M.Range(lt=150.0))])
        return self.client.count(name, count_filter=flt, exact=True).count

    def count_legal(self, name: str) -> int:
        flt = M.Filter(must=[M.FieldCondition(key="year", range=M.Range(gte=2010))])
        return self.client.count(name, count_filter=flt, exact=True).count

    def facet_district(self, name: str) -> dict:
        hits = self.client.facet(name, key="district", limit=20, exact=True).hits
        return {h.value: h.count for h in hits}

    # ----------------------------------------------------------- writes ----
    def update_payload(self, name: str) -> None:
        flt = M.Filter(must=[M.FieldCondition(key="district", match=M.MatchValue(value="Mitte"))])
        self.client.set_payload(name, payload={"rating": 4.5}, points=flt, wait=True)

    def delete_by_filter(self, name: str) -> None:
        flt = M.Filter(must=[M.FieldCondition(key="price", range=M.Range(gt=250.0))])
        self.client.delete(name, points_selector=M.FilterSelector(filter=flt), wait=True)

    def prepared_rerun(self, name: str, qvecs: list[list[float]]) -> list:
        """No prepared statements in the official SDK: repeat the call."""
        hits = []
        for v in qvecs:
            hits = self.client.query_points(
                name, query=v, using="dense", limit=10, with_payload=True,
            ).points
        return hits
