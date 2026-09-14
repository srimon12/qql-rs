#!/usr/bin/env python3
"""Live E2E: pyqql (QQL) vs official qdrant-client against Qdrant 1.19.1.

Every scenario runs the SAME logical operation on IDENTICAL data over the SAME
REST transport (http://localhost:6333), with `wait=True` writes, and checks
RESULT PARITY (ids + scores, not just timing). Timing is warmup + median.

Covered: cold import, client construction, CREATE COLLECTION, UPSERT batch,
filtered vector search, prepared-statement reruns, SCROLL pagination, COUNT,
FACET, UPDATE payload, DELETE by filter.

Usage:
    python3 experiment/bench_e2e.py [--iters N] [--reps N] [--points N]

Requires: qdrant-client==1.19.0 (latest published; 1.19.1 was never released
as a Python package — 1.19.1 is the *server* version), pyqql 0.3.2, and a live
Qdrant at http://localhost:6333.
"""

from __future__ import annotations

import argparse
import inspect
import statistics
import subprocess
import sys
import time

# -----------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------

def median(xs):
    return statistics.median(xs)


def time_op(fn, warmup, iters):
    """Median seconds/op over one run of `iters` (after `warmup` untimed)."""
    for _ in range(warmup):
        fn()
    start = time.perf_counter()
    for _ in range(iters):
        fn()
    return (time.perf_counter() - start) / iters


def cold_import(package, reps=3):
    """Honest boot time: fresh interpreter per rep."""
    samples = []
    for _ in range(reps):
        start = time.perf_counter()
        subprocess.run(
            [sys.executable, "-c", f"import {package}"],
            check=True,
            capture_output=True,
        )
        samples.append(time.perf_counter() - start)
    return median(samples)


# -----------------------------------------------------------------
# Fixture data (deterministic so both sides ingest identical points)
# -----------------------------------------------------------------

CATS = ["tech", "science", "art"]


def vec(i):
    return [((i * 7 + j * 13) % 100) / 100.0 for j in range(4)]


def points(n):
    from qdrant_client.models import PointStruct

    return [
        PointStruct(
            id=i,
            vector=vec(i),
            payload={"category": CATS[i % 3], "value": i},
        )
        for i in range(n)
    ]


def upsert_vals(n):
    return ", ".join(
        "{id: %d, vector: [%s], category: '%s', value: %d}"
        % (i, ", ".join(repr(x) for x in vec(i)), CATS[i % 3], i)
        for i in range(n)
    )


# -----------------------------------------------------------------
# The two contenders, side by side (also the LOC showdown input)
# -----------------------------------------------------------------

def sdk_filtered_search(client, collection, query_vec):
    from qdrant_client.models import FieldCondition, Filter, MatchValue

    return client.query_points(
        collection_name=collection,
        query=query_vec,
        query_filter=Filter(
            must=[FieldCondition(key="category", match=MatchValue(value="tech"))]
        ),
        limit=10,
    ).points


QQL_FILTERED_SEARCH = "QUERY %s FROM %s WHERE category = 'tech' LIMIT 10"


def main():
    ap = argparse.ArgumentParser(description="pyqql vs qdrant-client live E2E")
    ap.add_argument("--iters", type=int, default=100, help="search/prepared iterations")
    ap.add_argument("--reps", type=int, default=3, help="median-of reps for read scenarios")
    ap.add_argument("--points", type=int, default=1000, help="fixture point count")
    args = ap.parse_args()
    n = args.points

    import qdrant_client
    from qdrant_client import QdrantClient
    from qdrant_client.models import (
        Distance,
        FieldCondition,
        Filter,
        MatchValue,
        Range,
        VectorParams,
    )
    import pyqql

    assert pyqql.__version__ >= "0.3", f"stale pyqql shadowing sys.path? {pyqql.__file__}"
    print("pyqql %s from %s" % (pyqql.__version__, pyqql.__file__), file=sys.stderr)

    OFF, QQL = "bench_official", "bench_qql"
    results, parity = {}, {}

    # -- 0. cold import -------------------------------------------------
    results["cold import"] = (
        cold_import("qdrant_client") * 1000,
        cold_import("pyqql") * 1000,
    )

    # -- 1. construction ------------------------------------------------
    results["client construct"] = (
        median([_t(QdrantClient, url="http://localhost:6333") for _ in range(5)]) * 1000,
        median([_t(pyqql.Client, url="http://localhost:6333", use_grpc=False) for _ in range(5)]) * 1000,
    )

    sdk = QdrantClient(url="http://localhost:6333")
    qql = pyqql.Client(url="http://localhost:6333", use_grpc=False)
    try:
        for c in (OFF, QQL):
            if sdk.collection_exists(c):
                sdk.delete_collection(c)

        # -- 2. create --------------------------------------------------
        t0 = time.perf_counter()
        sdk.create_collection(
            collection_name=OFF, vectors_config=VectorParams(size=4, distance=Distance.COSINE)
        )
        t_sdk = time.perf_counter() - t0
        t0 = time.perf_counter()
        qql.execute("CREATE COLLECTION %s (dense VECTOR(4, COSINE))" % QQL)
        t_qql = time.perf_counter() - t0
        results["create collection"] = (t_sdk * 1000, t_qql * 1000)

        # -- 3. upsert --------------------------------------------------
        data = points(n)
        t0 = time.perf_counter()
        sdk.upsert(collection_name=OFF, points=data, wait=True)
        t_sdk = time.perf_counter() - t0
        t0 = time.perf_counter()
        qql.execute("UPSERT INTO %s VALUES %s" % (QQL, upsert_vals(n)))
        t_qql = time.perf_counter() - t0
        co = sdk.count(collection_name=OFF, exact=True).count
        cq = qql.execute("COUNT FROM %s" % QQL).count()
        parity["upsert count"] = co == cq == n
        results[f"upsert ({n} pts)"] = (t_sdk * 1000, t_qql * 1000)

        # -- 3b. payload index (facet requires one server-side) -----------
        t0 = time.perf_counter()
        sdk.create_payload_index(
            collection_name=OFF, field_name="category", field_schema="keyword"
        )
        t_sdk = time.perf_counter() - t0
        t0 = time.perf_counter()
        qql.execute(
            "CREATE INDEX ON COLLECTION %s FOR category TYPE keyword" % QQL
        )
        t_qql = time.perf_counter() - t0
        results["create index"] = (t_sdk * 1000, t_qql * 1000)
        # Index build is async: wait (untimed) until both sides serve it.
        deadline = time.perf_counter() + 60
        while time.perf_counter() < deadline:
            schemas = [
                sdk.get_collection(c).payload_schema or {} for c in (OFF, QQL)
            ]
            if all("category" in s for s in schemas):
                break
            time.sleep(0.2)
        else:
            raise RuntimeError("payload index never became ready")

        # -- 4. filtered search -----------------------------------------
        qv = vec(7)
        filt = Filter(must=[FieldCondition(key="category", match=MatchValue(value="tech"))])
        qq = QQL_FILTERED_SEARCH % ("[%s]" % ", ".join(repr(x) for x in qv), QQL)
        sdk_hits = sdk_filtered_search(sdk, OFF, qv)
        qql_hits = qql.execute(qq).hits()
        parity["search parity"] = parity_topk(sdk_hits, qql_hits)
        sdk_ms = median(
            [time_op(lambda: sdk_filtered_search(sdk, OFF, qv), 10, args.iters) for _ in range(args.reps)]
        ) * 1000
        qql_ms = median(
            [time_op(lambda: qql.execute(qq), 10, args.iters) for _ in range(args.reps)]
        ) * 1000
        results[f"search ({args.iters}x{args.reps} med)"] = (sdk_ms, qql_ms)

        # -- 5. prepared reruns (the QQL power move) --------------------
        variants = [(vec(i * 11), CATS[i % 3]) for i in range(5)]
        template = pyqql.parse(
            "QUERY :qv FROM %s USING dense WHERE category = :cat LIMIT 10" % QQL
        )[0]
        sdk_prep = median([
            time_op(
                lambda: [
                    sdk_filtered_search_cat(sdk, OFF, v, c) for v, c in variants
                ],
                2,
                20,
            )
            for _ in range(args.reps)
        ]) * 1000 / 5
        qql_prep = median([
            time_op(
                lambda: [qql.execute(template.bind({"qv": v, "cat": c})) for v, c in variants],
                2,
                20,
            )
            for _ in range(args.reps)
        ]) * 1000 / 5
        # parity on one variant
        v0, c0 = variants[0]
        parity["prepared parity"] = parity_topk(
            sdk_filtered_search_cat(sdk, OFF, v0, c0),
            qql.execute(template.bind({"qv": v0, "cat": c0})).hits(),
        )
        results["prepared rerun /op"] = (sdk_prep, qql_prep)

        # -- 6. scroll pagination ---------------------------------------
        def sdk_scroll_all():
            ids, off = [], None
            while True:
                recs, off = sdk.scroll(collection_name=OFF, limit=250, offset=off)
                ids.extend(r.id for r in recs)
                if off is None:
                    return ids

        def qql_scroll_all():
            # NOTE: QQL AFTER is inclusive (page boundary repeats), while the
            # official offset is exclusive — drop the repeated head per page,
            # but test the RAW page size for end-of-data (a trimmed full page
            # is one short and would stop pagination early).
            ids, after = [], None
            while True:
                if after is None:
                    q = "SCROLL FROM %s LIMIT 250" % QQL
                else:
                    q = "SCROLL FROM %s AFTER %s LIMIT 250" % (QQL, after)
                raw = qql.execute(q).hits()
                if not raw:
                    return ids
                batch = raw[1:] if after is not None else raw
                ids.extend(h.id for h in batch)
                after = raw[-1].id
                if len(raw) < 250:
                    return ids

        s_ids, q_ids = sdk_scroll_all(), qql_scroll_all()
        parity["scroll parity"] = sorted(s_ids) == sorted(q_ids) == list(range(n))
        results["scroll full scan"] = (
            median([time_op(sdk_scroll_all, 1, 3) for _ in range(args.reps)]) * 1000,
            median([time_op(qql_scroll_all, 1, 3) for _ in range(args.reps)]) * 1000,
        )

        # -- 7. count + facet -------------------------------------------
        sdk_c = sdk.count(collection_name=OFF, count_filter=filt, exact=True).count
        qql_c = qql.execute(
            "COUNT FROM %s WHERE category = 'tech'" % QQL
        ).count()
        parity["count parity"] = sdk_c == qql_c
        results["count"] = (
            median([time_op(lambda: sdk.count(collection_name=OFF, count_filter=filt), 3, 20) for _ in range(args.reps)]) * 1000,
            median([time_op(lambda: qql.execute("COUNT FROM %s WHERE category = 'tech'" % QQL), 3, 20) for _ in range(args.reps)]) * 1000,
        )
        sdk_f = {h.value: h.count for h in sdk.facet(collection_name=OFF, key="category", limit=10).hits}
        qql_f = facet_map(qql.execute("FACET category FROM %s LIMIT 10" % QQL).facet())
        parity["facet parity"] = sdk_f == qql_f
        results["facet"] = (
            median([time_op(lambda: sdk.facet(collection_name=OFF, key="category", limit=10), 3, 20) for _ in range(args.reps)]) * 1000,
            median([time_op(lambda: qql.execute("FACET category FROM %s LIMIT 10" % QQL), 3, 20) for _ in range(args.reps)]) * 1000,
        )

        # -- 8. update payload ------------------------------------------
        uf = Filter(must=[FieldCondition(key="value", range=Range(gte=990))])
        sdk_up = time_op(
            lambda: sdk.set_payload(collection_name=OFF, payload={"reviewed": True}, points=uf, wait=True),
            0, 1,
        )
        qql_up = time_op(
            lambda: qql.execute("UPDATE %s SET PAYLOAD = {reviewed: true} WHERE value >= 990" % QQL),
            0, 1,
        )
        chk = Filter(must=[FieldCondition(key="reviewed", match=MatchValue(value=True))])
        parity["update parity"] = (
            sdk.count(collection_name=OFF, count_filter=chk, exact=True).count
            == qql.execute("COUNT FROM %s WHERE reviewed = true" % QQL).count()
            == 10
        )
        # reset reviewed flag on both for a clean delete comparison
        sdk.set_payload(collection_name=OFF, payload={"reviewed": None}, points=chk, wait=True)
        results["update payload"] = (sdk_up * 1000, qql_up * 1000)

        # -- 9. delete by filter ----------------------------------------
        df = Filter(must=[FieldCondition(key="category", match=MatchValue(value="art"))])
        sdk_dl = time_op(lambda: sdk.delete(collection_name=OFF, points_selector=df, wait=True), 0, 1)
        # re-create QQL side? No — delete on both once; time each on its own collection.
        qql_dl = time_op(
            lambda: qql.execute("DELETE FROM %s WHERE category = 'art'" % QQL), 0, 1
        )
        parity["delete parity"] = (
            sdk.count(collection_name=OFF, exact=True).count
            == qql.execute("COUNT FROM %s" % QQL).count()
        )
        results["delete by filter"] = (sdk_dl * 1000, qql_dl * 1000)
    finally:
        for c in (OFF, QQL):
            try:
                sdk.delete_collection(c)
            except Exception:
                pass

    # -- report ----------------------------------------------------------
    sdk_loc = len(inspect.getsource(sdk_filtered_search).strip().splitlines())
    qql_loc = len(QQL_FILTERED_SEARCH.strip().splitlines())
    print("\n" + "=" * 64)
    print("pyqql %s vs qdrant-client %s  |  live Qdrant %s" % (
        pyqql.__version__, qdrant_client.__version__ if hasattr(qdrant_client, "__version__") else "1.19.0",
        "1.19.1",
    ))
    print("=" * 64)
    print(f"{'Scenario':<24} | {'official':>12} | {'pyqql':>12} | {'winner':>8}")
    print("-" * 64)
    for name, (a, b) in results.items():
        win = "pyqql" if b < a else "official"
        print(f"{name:<24} | {a:>9.2f} ms | {b:>9.2f} ms | {win:>8}")
    print("-" * 64)
    print("Parity (identical results, not just timing):")
    for name, ok in parity.items():
        print(f"  [{'OK' if ok else 'MISMATCH'}] {name}")
    print("-" * 64)
    print(f"Filtered-search code: SDK {sdk_loc} lines vs QQL {qql_loc} line:")
    print(f"  {QQL_FILTERED_SEARCH % ('[0.1, 0.2, 0.3, 0.4]', 'docs')}")
    print("Note: create-index is ack-vs-wait — the official call blocks until")
    print("the index builds, QQL acks on submit (readiness polled, untimed,")
    print("before facet). All other rows are like-for-like.")
    print("=" * 64)


def _t(fn, *a, **k):
    start = time.perf_counter()
    fn(*a, **k)
    return time.perf_counter() - start


def parity_topk(sdk_hits, qql_hits, tol=1e-4):
    """Tie-aware top-k parity: identical score distributions plus identical
    membership above the cutoff. Scores tie across duplicate vectors, so the
    boundary member may legitimately differ between backends."""
    ss = sorted((h.score for h in sdk_hits), reverse=True)
    qs = sorted((h.score for h in qql_hits), reverse=True)
    if len(ss) != len(qs):
        return False
    if any(abs(a - b) > tol for a, b in zip(ss, qs)):
        return False
    cut = min(ss)
    s_strict = {h.id for h in sdk_hits if h.score - cut > tol}
    q_strict = {h.id for h in qql_hits if h.score - cut > tol}
    return s_strict == q_strict


def facet_map(hits):
    """Normalize QQL facet hits (dicts or objects) to {value: count}."""
    out = {}
    for h in hits:
        if isinstance(h, dict):
            out[h["value"]] = h["count"]
        elif hasattr(h, "value"):
            out[h.value] = h.count
        else:
            raise TypeError(f"unexpected facet hit shape: {h!r}")
    return out


def sdk_filtered_search_cat(client, collection, query_vec, cat):
    from qdrant_client.models import FieldCondition, Filter, MatchValue

    return client.query_points(
        collection_name=collection,
        query=query_vec,
        query_filter=Filter(
            must=[FieldCondition(key="category", match=MatchValue(value=cat))]
        ),
        limit=10,
    ).points


if __name__ == "__main__":
    main()
