"""qdrant-edge-py 0.8.0 scenario implementations.

This is the code a Qdrant Edge Python user writes against the official
bindings: explicit Point/QueryRequest/Filter graphs, no query language,
no embedder, no batch helpers.

Read methods build the request inside the timed call (matching the
vs-qdrant official SDK convention); ingest reports data preparation and
submission separately because qdrant-edge-py needs fully materialized
Python vectors (nested lists for multivectors).
"""
from __future__ import annotations

import time
from pathlib import Path

from qdrant_edge import (
    Bm25,
    CountRequest,
    Distance,
    EdgeConfig,
    EdgeShard,
    EdgeSparseVectorParams,
    EdgeVectorParams,
    FacetRequest,
    FieldCondition,
    Filter,
    Fusion,
    Modifier,
    MultiVectorComparator,
    MultiVectorConfig,
    PayloadSchemaType,
    Point,
    Prefetch,
    Query,
    QueryRequest,
    RangeFloat,
    ScrollRequest,
    SparseVector,
    UpdateOperation,
)

from config import BATCH_BERLIN, BATCH_LEGAL, HF_MODEL, LIMIT

_STATE: dict[str, EdgeShard] = {}


class OfficialEdge:
    """The official qdrant-edge-py bindings, one EdgeShard per collection."""

    def __init__(self, base: Path):
        self.base = Path(base)
        self.base.mkdir(parents=True, exist_ok=True)
        self.embedder = None

    def load_embedder(self) -> None:
        """Load the official-side dense embedder (fastembed Python, CPU)."""
        from fastembed import TextEmbedding

        self.embedder = TextEmbedding(HF_MODEL, providers=["CPUExecutionProvider"])

    def text_dense(self, key: str, text: str) -> list:
        qvec = list(self.embedder.embed([text]))[0].tolist()
        return self.query_dense(key, qvec)

    def text_sparse(self, key: str, text: str) -> list:
        svec = Bm25().embed_query(text)
        return _STATE[key].query(
            QueryRequest(
                limit=LIMIT,
                query=Query.Nearest(svec, using="bm25"),
                with_payload=True,
                with_vector=False,
            )
        )

    # ------------------------------------------------------------ setup ----
    def create_berlin(self, key: str) -> None:
        path = self.base / key
        path.mkdir(parents=True, exist_ok=True)
        shard = EdgeShard.create(
            str(path),
            EdgeConfig(
                vectors={"dense": EdgeVectorParams(size=384, distance=Distance.Cosine)},
                sparse_vectors={
                    "bm25": EdgeSparseVectorParams(modifier=Modifier.Idf)
                },
                on_disk_payload=False,
            ),
        )
        shard.update(
            UpdateOperation.create_field_index("district", PayloadSchemaType.Keyword)
        )
        shard.update(
            UpdateOperation.create_field_index("price", PayloadSchemaType.Float)
        )
        _STATE[key] = shard

    def create_legal(self, key: str) -> None:
        path = self.base / key
        path.mkdir(parents=True, exist_ok=True)
        shard = EdgeShard.create(
            str(path),
            EdgeConfig(
                vectors={
                    "dense": EdgeVectorParams(size=384, distance=Distance.Cosine),
                    "colbert": EdgeVectorParams(
                        size=128,
                        distance=Distance.Cosine,
                        multivector_config=MultiVectorConfig(
                            MultiVectorComparator.MaxSim
                        ),
                    ),
                },
                sparse_vectors={
                    "bm25": EdgeSparseVectorParams(modifier=Modifier.Idf)
                },
                on_disk_payload=False,
            ),
        )
        shard.update(
            UpdateOperation.create_field_index("court", PayloadSchemaType.Keyword)
        )
        shard.update(
            UpdateOperation.create_field_index("year", PayloadSchemaType.Integer)
        )
        _STATE[key] = shard

    def close(self) -> None:
        for shard in _STATE.values():
            shard.close()
        _STATE.clear()

    def load(self, key: str) -> None:
        _STATE[key] = EdgeShard.load(str(self.base / key))

    # ----------------------------------------------------------- ingest ----
    def ingest_berlin(self, key: str, docs, dense, sparse) -> tuple[float, float]:
        dense_list = dense.tolist()
        t0 = time.perf_counter()
        points = [
            Point(
                id=doc["id"],
                vector={
                    "dense": dense_list[k],
                    "bm25": SparseVector(
                        indices=sparse[k]["indices"], values=sparse[k]["values"]
                    ),
                },
                payload={k: v for k, v in doc.items() if k != "id"},
            )
            for k, doc in enumerate(docs)
        ]
        prep = time.perf_counter() - t0

        t0 = time.perf_counter()
        for i in range(0, len(points), BATCH_BERLIN):
            _STATE[key].update(
                UpdateOperation.upsert_points(points[i : i + BATCH_BERLIN])
            )
        return prep, time.perf_counter() - t0

    def ingest_legal(
        self, key: str, docs, dense, sparse, colbert_flat, colbert_lens
    ) -> tuple[float, float]:
        dense_list = dense.tolist()
        t0 = time.perf_counter()
        # Multivectors must be materialized Python lists-of-lists: no flat
        # {data, dim} convention exists in the official bindings.
        colbert, offset = [], 0
        for n in colbert_lens:
            colbert.append(
                colbert_flat[offset : offset + n * 128].reshape(n, 128).tolist()
            )
            offset += n * 128
        points = [
            Point(
                id=doc["id"],
                vector={
                    "dense": dense_list[k],
                    "bm25": SparseVector(
                        indices=sparse[k]["indices"], values=sparse[k]["values"]
                    ),
                    "colbert": colbert[k],
                },
                payload={k: v for k, v in doc.items() if k != "id"},
            )
            for k, doc in enumerate(docs)
        ]
        prep = time.perf_counter() - t0

        t0 = time.perf_counter()
        for i in range(0, len(points), BATCH_LEGAL):
            _STATE[key].update(UpdateOperation.upsert_points(points[i : i + BATCH_LEGAL]))
        return prep, time.perf_counter() - t0

    # ------------------------------------------------------------- reads ----
    def query_dense(self, key: str, qvec) -> list:
        return _STATE[key].query(
            QueryRequest(
                limit=LIMIT,
                query=Query.Nearest(qvec, using="dense"),
                with_payload=True,
                with_vector=False,
            )
        )

    def query_dense_filtered(self, key: str, qvec) -> list:
        flt = Filter(
            must=[
                FieldCondition(key="price", range=RangeFloat(lt=150.0)),
                FieldCondition(key="guests", range=RangeFloat(gte=2.0)),
            ]
        )
        return _STATE[key].query(
            QueryRequest(
                limit=LIMIT,
                query=Query.Nearest(qvec, using="dense"),
                filter=flt,
                with_payload=True,
                with_vector=False,
            )
        )

    def query_sparse(self, key: str, svec) -> list:
        return _STATE[key].query(
            QueryRequest(
                limit=LIMIT,
                query=Query.Nearest(
                    SparseVector(indices=svec["indices"], values=svec["values"]),
                    using="bm25",
                ),
                with_payload=True,
                with_vector=False,
            )
        )

    def query_hybrid(self, key: str, qvec, svec) -> list:
        return _STATE[key].query(
            QueryRequest(
                limit=LIMIT,
                query=Fusion.Rrf(k=2),
                prefetches=[
                    Prefetch(limit=50, query=Query.Nearest(qvec, using="dense")),
                    Prefetch(
                        limit=50,
                        query=Query.Nearest(
                            SparseVector(
                                indices=svec["indices"], values=svec["values"]
                            ),
                            using="bm25",
                        ),
                    ),
                ],
                with_payload=True,
                with_vector=False,
            )
        )

    def query_colbert(self, key: str, mvec) -> list:
        return _STATE[key].query(
            QueryRequest(
                limit=LIMIT,
                query=Query.Nearest(mvec, using="colbert"),
                with_payload=True,
                with_vector=False,
            )
        )

    def count_berlin(self, key: str) -> int:
        flt = Filter(must=[FieldCondition(key="price", range=RangeFloat(lt=150.0))])
        return _STATE[key].count(CountRequest(exact=True, filter=flt))

    def count_legal(self, key: str) -> int:
        return _STATE[key].count(
            CountRequest(
                exact=True,
                filter=Filter(
                    must=[FieldCondition(key="year", range=RangeFloat(gte=2010.0))]
                ),
            )
        )

    def facet_district(self, key: str) -> dict:
        response = _STATE[key].facet(
            FacetRequest(key="district", limit=20, exact=True)
        )
        return {hit.value: hit.count for hit in response.hits}

    def optimize(self, key: str) -> bool:
        return _STATE[key].optimize()

    def scroll_pages(self, key: str, pages: int, batch: int) -> list:
        ids, offset = [], None
        for _ in range(pages):
            points, offset = _STATE[key].scroll(
                ScrollRequest(offset=offset, limit=batch, with_payload=True)
            )
            ids.extend(point.id for point in points)
            if offset is None:
                break
        return ids

    def retrieve_points(self, key: str, ids: list[int]) -> list:
        return _STATE[key].retrieve(ids, with_payload=True, with_vector=False)

    def prepared_rerun(self, key: str, qvecs) -> list:
        """No prepared statements in the official bindings: rebuild per call."""
        hits = []
        for qvec in qvecs:
            hits = self.query_dense(key, qvec)
        return hits

    def batch_reads(self, key: str, qvecs) -> list:
        """No statement batching: N queries are N Python calls."""
        return [hit for qvec in qvecs for hit in self.query_dense(key, qvec)]
