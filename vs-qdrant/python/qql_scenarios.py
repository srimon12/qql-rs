"""QQL SDK (pyqql) scenario implementations.

One method per benchmark scenario — the exact application code a QQL user
writes, nothing else (no harness/timing/parity). The symmetry with
official_scenarios.py is the LOC comparison.

Notes:
  * QUERY parameters (:dv / :sv / :mv) are bound per call — parse once, rerun
    with different vectors (prepared statements).
  * Bulk ingest is one call — upsert_many prepares a `:rows` template once
    and chunks point dicts internally (payload as data, never SQL text).
"""
from __future__ import annotations

import time

import pyqql

from config import BATCH_BERLIN, BATCH_LEGAL, LIMIT, SCROLL_BATCH

# QQL strings are JSON-compatible (double-quoted, backslash escapes) but do
# not decode \uXXXX — keep unicode literal (QQL text is UTF-8, same as the
# wire format the official SDK sends).


class QqlScenarios:
    """pyqql 0.4.0 — the QQL Python SDK."""

    def __init__(self, url: str):
        self.client = pyqql.Client(url)
        self.parse = pyqql.parse

    def close(self) -> None:
        self.client.close()

    # ------------------------------------------------------------ setup ----
    def drop_collection(self, name: str) -> None:
        try:
            self.client.execute(f"DROP COLLECTION {name}")
        except pyqql.QqlError:
            pass

    def create_berlin(self, name: str) -> None:
        self.client.execute(f"""
            CREATE COLLECTION {name} (
                dense VECTOR(384, COSINE),
                bm25 SPARSE
            )""")
        self.client.execute(f"CREATE INDEX ON COLLECTION {name} FOR district TYPE keyword")
        self.client.execute(f"CREATE INDEX ON COLLECTION {name} FOR price TYPE float")

    def create_legal(self, name: str) -> None:
        self.client.execute(f"""
            CREATE COLLECTION {name} (
                dense VECTOR(384, COSINE),
                bm25 SPARSE,
                colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH HNSW (m = 0)
            )""")
        self.client.execute(f"CREATE INDEX ON COLLECTION {name} FOR court TYPE keyword")
        self.client.execute(f"CREATE INDEX ON COLLECTION {name} FOR year TYPE integer")

    # ----------------------------------------------------------- ingest ----
    def ingest_berlin(self, name: str, docs, dense, sparse) -> float:
        dense_list = dense.tolist() if hasattr(dense, "tolist") else dense
        t0 = time.perf_counter()
        rows = [
            {**doc, "vector": {"dense": dense_list[k], "bm25": sparse[k]}}
            for k, doc in enumerate(docs)
        ]
        self.client.upsert_many(name, rows, batch_size=BATCH_BERLIN)
        return time.perf_counter() - t0

    def ingest_legal(self, name: str, docs, dense, sparse, colbert_flat, colbert_lens) -> float:
        offsets = [0]
        for n in colbert_lens:
            offsets.append(offsets[-1] + n * 128)
        dense_list = dense.tolist() if hasattr(dense, "tolist") else dense
        t0 = time.perf_counter()
        rows = [
            {
                **doc,
                "vector": {
                    "dense": dense_list[k],
                    "bm25": sparse[k],
                    "colbert": {
                        "data": colbert_flat[offsets[k] : offsets[k + 1]].tolist()
                        if hasattr(colbert_flat, "tolist")
                        else colbert_flat[offsets[k] : offsets[k + 1]],
                        "dim": 128,
                    },
                },
            }
            for k, doc in enumerate(docs)
        ]
        self.client.upsert_many(name, rows, batch_size=BATCH_LEGAL)
        return time.perf_counter() - t0

    # ------------------------------------------------------------- reads ----
    def query_dense(self, name: str, qvec: list[float]) -> list:
        return self.client.execute(
            f"QUERY :dv FROM {name} USING dense LIMIT {LIMIT}",
            params={"dv": qvec},
        ).hits()

    def query_dense_filtered(self, name: str, qvec: list[float]) -> list:
        return self.client.execute(
            f"QUERY :dv FROM {name} USING dense "
            f"WHERE price < 150.0 AND guests >= 2 LIMIT {LIMIT}",
            params={"dv": qvec},
        ).hits()

    def query_sparse(self, name: str, svec: dict) -> list:
        return self.client.execute(
            f"QUERY :sv FROM {name} USING bm25 LIMIT {LIMIT}",
            params={"sv": svec},
        ).hits()

    def query_hybrid(self, name: str, qvec: list[float], svec: dict) -> list:
        # hnsw_ef=128 on the dense CTE: fused rankings must be deterministic
        # across the two independently built collections.
        return self.client.execute(f"""
            WITH d AS (QUERY :dv FROM {name} USING dense PARAMS (hnsw_ef = 128) LIMIT 50),
                 s AS (QUERY :sv FROM {name} USING bm25 LIMIT 50)
            QUERY FUSION RRF FROM {name} PREFETCH (d, s) LIMIT {LIMIT}""",
            params={"dv": qvec, "sv": svec},
        ).hits()

    def query_colbert(self, name: str, mvec: list) -> list:
        return self.client.execute(
            f"QUERY :mv FROM {name} USING colbert LIMIT {LIMIT}",
            params={"mv": mvec},
        ).hits()

    def scroll_pages(self, name: str, pages: int, batch: int) -> list[int]:
        ids, offset = [], None
        for _ in range(pages):
            sql = f"SCROLL FROM {name} AFTER {offset} LIMIT {batch}" if offset else f"SCROLL FROM {name} LIMIT {batch}"
            rep = self.client.execute(sql)
            page_ids = rep.ids()
            if not page_ids:
                break
            ids.extend(page_ids)
            offset = ids[-1]
        return ids

    def count_berlin(self, name: str) -> int:
        return self.client.execute(f"COUNT FROM {name} WHERE price < 150.0").count()

    def count_legal(self, name: str) -> int:
        return self.client.execute(f"COUNT FROM {name} WHERE year >= 2010").count()

    def facet_district(self, name: str) -> dict:
        hits = self.client.execute(f"FACET district FROM {name} LIMIT 20 EXACT true").facet()
        return {h["value"]: h["count"] for h in hits}

    # ----------------------------------------------------------- writes ----
    def update_payload(self, name: str) -> None:
        self.client.execute(
            f"UPDATE {name} SET PAYLOAD = {{'rating': 4.5}} "
            f"WHERE district = 'Mitte' WAIT true")

    def delete_by_filter(self, name: str) -> None:
        self.client.execute(f"DELETE FROM {name} WHERE price > 250.0 WAIT true")

    def prepared_rerun(self, name: str, qvecs: list[list[float]]) -> list:
        # Parse once, rerun with different vectors — the prepared path.
        stmt = self.parse(f"QUERY :dv FROM {name} USING dense LIMIT {LIMIT}")[0]
        hits = []
        for v in qvecs:
            hits = self.client.execute(stmt, params={"dv": v}).hits()
        return hits
