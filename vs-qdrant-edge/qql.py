"""pyqql-edge 0.4.0 scenario implementations.

The QQL user writes SQL strings (or prepares one statement and re-binds
params); vectors can be passed as flat numpy-backed lists, sparse dicts or
`{data, dim}` multivectors, and FastEmbed resolution is available in-process.

Every method mirrors official.py one-to-one for the LOC comparison.
"""
from __future__ import annotations

import json
import sys
import time
from pathlib import Path

from config import (
    BATCH_BERLIN,
    BATCH_LEGAL,
    LIMIT,
    MODEL,
    PYQQ_EDGE_PKG,
)

sys.path.insert(0, str(PYQQ_EDGE_PKG))
import pyqql_edge  # noqa: E402


class QqlEdge:
    """The QQL edge client — qdrant-edge + FastEmbed behind one statement API."""

    def __init__(self, base: Path):
        base = Path(base)
        base.mkdir(parents=True, exist_ok=True)
        self.client = pyqql_edge.local_executor(
            str(base), on_disk_payload=False, model=MODEL
        )

    def close(self) -> None:
        self.client.close()

    # ------------------------------------------------------------ setup ----
    def create_berlin(self, key: str) -> None:
        self.client.execute(
            f"CREATE COLLECTION {key} (dense VECTOR(384, COSINE), bm25 SPARSE)"
        )
        self.client.execute(
            f"CREATE INDEX ON COLLECTION {key} FOR district TYPE keyword"
        )
        self.client.execute(f"CREATE INDEX ON COLLECTION {key} FOR price TYPE float")

    def create_legal(self, key: str) -> None:
        self.client.execute(
            f"CREATE COLLECTION {key} ("
            "dense VECTOR(384, COSINE), bm25 SPARSE, "
            "colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim'))"
        )
        self.client.execute(f"CREATE INDEX ON COLLECTION {key} FOR court TYPE keyword")
        self.client.execute(f"CREATE INDEX ON COLLECTION {key} FOR year TYPE integer")

    # ----------------------------------------------------------- ingest ----
    def ingest_berlin(self, key: str, docs, dense, sparse) -> tuple[float, float]:
        t0 = time.perf_counter()
        rows = [
            {**doc, "vector": {"dense": dense[k], "bm25": sparse[k]}}
            for k, doc in enumerate(docs)
        ]
        prep = time.perf_counter() - t0

        t0 = time.perf_counter()
        self.client.upsert_many(key, rows, batch_size=BATCH_BERLIN)
        return prep, time.perf_counter() - t0

    def ingest_legal(
        self, key: str, docs, dense, sparse, colbert_flat, colbert_lens
    ) -> tuple[float, float]:
        t0 = time.perf_counter()
        # Flat {data, dim} multivector with the numpy buffer left intact:
        # pyqql binds 1-D float buffers as F32Array with a single memcpy.
        rows, offset = [], 0
        for k, doc in enumerate(docs):
            n = colbert_lens[k]
            rows.append(
                {
                    **doc,
                    "vector": {
                        "dense": dense[k],
                        "bm25": sparse[k],
                        "colbert": {
                            "data": colbert_flat[offset : offset + n * 128],
                            "dim": 128,
                        },
                    },
                }
            )
            offset += n * 128
        prep = time.perf_counter() - t0

        t0 = time.perf_counter()
        self.client.upsert_many(key, rows, batch_size=BATCH_LEGAL)
        return prep, time.perf_counter() - t0

    # ------------------------------------------------------------- reads ----
    def query_dense(self, key: str, qvec) -> list:
        return self.client.execute(
            f"QUERY :dv FROM {key} USING dense LIMIT {LIMIT}",
            params={"dv": qvec},
        ).hits()

    def query_dense_filtered(self, key: str, qvec) -> list:
        return self.client.execute(
            f"QUERY :dv FROM {key} USING dense "
            f"WHERE price < 150.0 AND guests >= 2 LIMIT {LIMIT}",
            params={"dv": qvec},
        ).hits()

    def query_sparse(self, key: str, svec) -> list:
        return self.client.execute(
            f"QUERY :sv FROM {key} USING bm25 LIMIT {LIMIT}",
            params={"sv": svec},
        ).hits()

    def query_hybrid(self, key: str, qvec, svec) -> list:
        return self.client.execute(
            f"""
            WITH d AS (QUERY :dv FROM {key} USING dense LIMIT 50),
                 s AS (QUERY :sv FROM {key} USING bm25 LIMIT 50)
            QUERY FUSION RRF FROM {key} PREFETCH (d, s) LIMIT {LIMIT}""",
            params={"dv": qvec, "sv": svec},
        ).hits()

    def query_colbert(self, key: str, mvec) -> list:
        return self.client.execute(
            f"QUERY :mv FROM {key} USING colbert LIMIT {LIMIT}",
            params={"mv": mvec},
        ).hits()

    def text_dense(self, key: str, text: str) -> list:
        return self.client.execute(
            f"QUERY TEXT {json.dumps(text)} FROM {key} USING dense LIMIT {LIMIT}"
        ).hits()

    def text_sparse(self, key: str, text: str) -> list:
        return self.client.execute(
            f"QUERY TEXT {json.dumps(text)} FROM {key} USING bm25 LIMIT {LIMIT}"
        ).hits()

    def count_berlin(self, key: str) -> int:
        return self.client.execute(
            f"COUNT FROM {key} WHERE price < 150.0"
        ).count()

    def count_legal(self, key: str) -> int:
        return self.client.execute(f"COUNT FROM {key} WHERE year >= 2010").count()

    def facet_district(self, key: str) -> dict:
        hits = self.client.execute(
            f"FACET district FROM {key} LIMIT 20 EXACT true"
        ).facet()
        return {hit["value"]: hit["count"] for hit in hits}

    def optimize(self, key: str) -> bool:
        return self.client.optimize(key)

    def scroll_pages(self, key: str, pages: int, batch: int) -> list:
        ids, offset = [], None
        for _ in range(pages):
            sql = (
                f"SCROLL FROM {key} AFTER {offset} LIMIT {batch}"
                if offset is not None
                else f"SCROLL FROM {key} LIMIT {batch}"
            )
            page_ids = self.client.execute(sql).ids()
            if not page_ids:
                break
            ids.extend(page_ids)
            offset = ids[-1]
        return ids

    def retrieve_points(self, key: str, ids: list[int]) -> list:
        joined = ", ".join(str(i) for i in ids)
        return self.client.execute(f"QUERY POINTS ({joined}) FROM {key}").hits()

    def prepared_rerun(self, key: str, qvecs) -> list:
        stmt = pyqql_edge.parse(f"QUERY :dv FROM {key} USING dense LIMIT {LIMIT}")[0]
        hits = []
        for qvec in qvecs:
            hits = self.client.execute(stmt, params={"dv": qvec}).hits()
        return hits

    def batch_reads(self, key: str, qvecs) -> list:
        """One call, N bound statements — the runtime batches the fan-out."""
        stmts = [
            pyqql_edge.parse(
                f"QUERY :dv FROM {key} USING dense LIMIT {LIMIT}"
            )[0].bind({"dv": qvec})
            for qvec in qvecs
        ]
        report = self.client.execute(stmts)
        return [hit for i in range(len(qvecs)) for hit in report.hits(i)]
