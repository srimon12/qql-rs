#!/usr/bin/env python3
"""Berlin sharded migration end-to-end, driven entirely through the ``qql`` CLI.

Proves, in order:

1. CREATE a berlin collection with the vs-qdrant demo config
   (dense 384/COSINE + bm25 SPARSE + district/price/guests indexes) plus
   production optimizations: Turbo 1.5-bit quantization (always_ram),
   HNSW (m=16, ef_construct=100) and a bulk-friendly indexing_threshold
   (20000 > 8000 points, so HNSW builds once at the end).
2. INGEST all 8000 berlin points purely through
   ``qql exec "UPSERT INTO <coll> VALUES :rows WAIT true"``. Python only
   shapes JSON params-files from the three corpus files; every write goes
   through the CLI binary.
3. VERIFY source reads (COUNT, FACET, dense/sparse/hybrid/filtered QUERY,
   SCROLL) via ``qql exec``.
4. DRY-RUN ``qql migrate`` (prints the plan: custom sharding, preserved
   quantization, 12 CREATE SHARD KEY statements).
5. MIGRATE src -> dst with ``--shard-key-field district`` on a multi-node
   cluster: FACET discovers the 12 district keys, the target is created
   with sharding_method='custom' + is_tenant index, each point is routed
   with ``SHARD '<district>'``.
6. VERIFY target + tenant isolation (COUNT/FACET parity, SHOW SHARD KEYS,
   SHARD-routed SCROLL/COUNT/FACET/QUERY/UPDATE, top-k parity).
7. DUMP the sharded target with ``qql dump`` and replay the script into a
   fresh collection with ``qql execute`` (round-trip proof for sharded
   dumps: CREATE SHARD KEY statements + SHARD-routed UPSERT batches).

Prerequisites: multi-node Qdrant on :6333/:7333/:8333, a built ``qql``
binary (``cargo build -p qql-cli``), corpus files under vs-qdrant/data/,
and stdlib-only python3 (no extra dependencies).

Usage:
    python3 berlin_shard_migration.py [--url URL] [--qql BIN] [--batch N]
        [--workers N] [--skip-ingest] [--skip-migrate] [--clean]
"""

from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

MIGRATE_DIR = Path(__file__).resolve().parent
ROOT = MIGRATE_DIR.parents[3]
DATA = ROOT / "vs-qdrant" / "data"
DIM = 384
POINTS = 8000

SRC = "vsq_berlin_src"
DST = "vsq_berlin_dst"
RST = "vsq_berlin_rst"
SHARD_FIELD = "district"


class Fail(Exception):
    """Assertion failure with a human-readable message."""


def say(step: str) -> None:
    print(f"\n== {step} ==", flush=True)


def ok(msg: str) -> None:
    print(f"PASS {msg}", flush=True)


def newest_qql(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit)
    candidates = [ROOT / "target" / "release" / "qql", ROOT / "target" / "debug" / "qql"]
    # Newest first: a stale binary predates the working tree (e.g. it can
    # miss the float-equality filter fix) and fails verification.
    found = sorted(
        (c for c in candidates if c.is_file() and c.stat().st_mode & 0o111),
        key=lambda c: c.stat().st_mtime,
        reverse=True,
    )
    if not found:
        raise Fail("qql binary not found; run `cargo build -p qql-cli` first")
    return found[0]


class Qql:
    """Thin wrapper: every cluster write/read goes through the CLI binary."""

    def __init__(self, binary: Path, url: str) -> None:
        self.binary = str(binary)
        self.url = url

    def run(
        self, *args: str, check: bool = True, live: bool = False
    ) -> subprocess.CompletedProcess[str]:
        # live=True inherits stdout/stderr so long commands (migrate, dump)
        # stream progress instead of going silent until they finish.
        proc = subprocess.run(
            [self.binary, "--url", self.url, *args],
            capture_output=not live,
            text=True,
        )
        if check and proc.returncode != 0:
            raise Fail(f"qql {' '.join(args)} failed:\n{proc.stdout}\n{proc.stderr}")
        return proc

    def exec(self, sql: str, params_file: str | None = None, quiet: bool = False) -> dict:
        args = ["exec", sql]
        if params_file:
            args += ["--params-file", params_file]
        if quiet:
            args += ["--quiet"]
        else:
            args += ["--json"]
        proc = self.run(*args)
        if quiet:
            return {}
        return json.loads(proc.stdout)

    def count(self, collection: str, where: str = "", shard: str | None = None) -> int:
        sql = f"COUNT FROM {collection}"
        if where:
            sql += f" WHERE {where}"
        if shard:
            sql += f" SHARD '{shard}'"
        data = self.exec(sql + ";")["results"][0]["data"]
        if not isinstance(data, dict) or "count" not in data:
            raise Fail(f"could not read count from {data}")
        return int(data["count"])


def check_prereqs(qql: Qql) -> None:
    say("0/7 prereqs")
    qql.run("doctor")
    cluster = json.loads(
        subprocess.run(
            ["curl", "-sf", f"{qql.url}/cluster"], capture_output=True, text=True, check=True
        ).stdout
    )
    peers = len(cluster["result"]["peers"])
    if peers < 2:
        raise Fail(f"need a multi-node cluster for shard keys, got {peers} peer(s)")
    ok(f"cluster has {peers} peers (distributed mode)")
    for name in ("berlin.jsonl", "berlin_dense.f32", "berlin_bm25.json"):
        if not (DATA / name).is_file():
            raise Fail(f"missing vs-qdrant/data/{name}")
    if (DATA / "berlin_dense.f32").stat().st_size != POINTS * DIM * 4:
        raise Fail("berlin_dense.f32 has an unexpected size")
    ok(f"corpus present ({POINTS} berlin docs)")


def create_source(qql: Qql) -> None:
    say(f"1/7 CREATE {SRC} (demo config + turbo-1.5 + bulk HNSW/optimizers)")
    qql.run("exec", f"DROP COLLECTION {SRC};", check=False)
    # Demo config mirrors vs-qdrant/python/qql_scenarios.py::create_berlin.
    qql.exec(
        f"CREATE COLLECTION {SRC} "
        "(dense VECTOR(384, COSINE) WITH HNSW (m = 16, ef_construct = 100), bm25 SPARSE) "
        "WITH QUANTIZATION (type = 'turbo', bits = 1.5, always_ram = true) "
        "WITH HNSW (m = 16, ef_construct = 100) "
        "WITH OPTIMIZERS (indexing_threshold = 20000);",
        quiet=True,
    )
    for field, kind in (("district", "keyword"), ("price", "float"), ("guests", "integer")):
        qql.exec(f"CREATE INDEX ON COLLECTION {SRC} FOR {field} TYPE {kind};", quiet=True)
    ok("source created (turbo-1.5 + indexing_threshold=20000)")


def load_corpus() -> tuple[list[dict], list[list[float]], list[dict]]:
    docs = [json.loads(line) for line in (DATA / "berlin.jsonl").read_text().splitlines()]
    raw = (DATA / "berlin_dense.f32").read_bytes()
    dense = [list(struct.unpack(f"<{DIM}f", raw[i * DIM * 4 : (i + 1) * DIM * 4])) for i in range(len(docs))]
    sparse = json.loads((DATA / "berlin_bm25.json").read_text())
    assert len(docs) == len(dense) == len(sparse) == POINTS
    return docs, dense, sparse


def ingest(qql: Qql, batch: int, tmp: Path) -> None:
    say(f"2/7 INGEST {POINTS} pts via 'qql exec UPSERT … VALUES :rows'")
    docs, dense, sparse = load_corpus()
    t0 = time.perf_counter()
    params_file = tmp / "batch.json"
    for start in range(0, len(docs), batch):
        rows = [
            {**docs[k], "vector": {"dense": dense[k], "bm25": sparse[k]}}
            for k in range(start, min(start + batch, len(docs)))
        ]
        params_file.write_text(json.dumps({"rows": rows}))
        qql.exec(f"UPSERT INTO {SRC} VALUES :rows WAIT true", str(params_file), quiet=True)
        if (start // batch) % 10 == 0:
            print(f"  {start + len(rows)}/{len(docs)}", flush=True)
    dt = time.perf_counter() - t0
    if qql.count(SRC) != POINTS:
        raise Fail("src count mismatch after ingest")
    ok(f"ingested {POINTS} pts in {dt:.1f}s ({POINTS / dt:.0f} pts/s)")


def write_query_vectors(tmp: Path) -> dict[str, Path]:
    docs, dense, sparse = load_corpus()
    files = {
        "dense": {"dv": dense[0]},
        "sparse": {"sv": sparse[0]},
        "hybrid": {"dv": dense[0], "sv": sparse[0]},
    }
    paths = {}
    for name, params in files.items():
        path = tmp / f"qvec_{name}.json"
        path.write_text(json.dumps(params))
        paths[name] = path
    return paths


def verify_source(qql: Qql, tmp: Path) -> None:
    say("3/7 VERIFY source reads (all via qql exec)")
    if qql.count(SRC) != POINTS:
        raise Fail("src count drifted")
    if qql.count(SRC, "price < 150.0") != 4198:
        raise Fail("filtered COUNT price<150 drifted (want 4198)")
    vecs = write_query_vectors(tmp)
    top = qql.exec(f"QUERY :dv FROM {SRC} USING dense LIMIT 3;", str(vecs["dense"]))["results"][0][
        "data"
    ]
    if top[0]["id"] != 1:
        raise Fail(f"doc-0 vector must return id 1 top-1, got {top[0]['id']}")
    qql.exec(f"QUERY :sv FROM {SRC} USING bm25 LIMIT 3;", str(vecs["sparse"]))
    qql.exec(
        f"WITH d AS (QUERY :dv FROM {SRC} USING dense PARAMS (hnsw_ef = 128) LIMIT 50), "
        f"s AS (QUERY :sv FROM {SRC} USING bm25 LIMIT 50) "
        f"QUERY FUSION RRF FROM {SRC} PREFETCH (d, s) LIMIT 3;",
        str(vecs["hybrid"]),
    )
    qql.exec(
        f"QUERY :dv FROM {SRC} USING dense WHERE price < 150.0 AND guests >= 2 LIMIT 3;",
        str(vecs["dense"]),
    )
    qql.exec(f"SCROLL FROM {SRC} LIMIT 3;")
    ok("source reads green")


def dry_run(qql: Qql) -> None:
    say("4/7 DRY-RUN migrate (plan only — expect 12 shard keys)")
    proc = qql.run("migrate", SRC, "--to", DST, "--shard-key-field", SHARD_FIELD, "--dry-run")
    print(proc.stdout)
    if proc.stdout.count("CREATE SHARD KEY") != 12:
        raise Fail("dry-run should discover 12 shard keys")


def migrate(qql: Qql, batch: int, workers: int) -> None:
    say(f"5/7 MIGRATE {SRC} -> {DST} with --shard-key-field {SHARD_FIELD}")
    t0 = time.perf_counter()
    qql.run(
        "migrate", SRC, "--to", DST, "--shard-key-field", SHARD_FIELD,
        "--recreate", "--restart", "--workers", str(workers), "--batch-size", str(batch),
        live=True,
    )
    print(f"migrate took {time.perf_counter() - t0:.0f}s", flush=True)


def facet_map(qql: Qql, collection: str) -> dict:
    data = qql.exec(f"FACET district FROM {collection} LIMIT 20 EXACT true;")["results"][0]["data"]
    return {hit["value"]: hit["count"] for hit in data}


def verify_target(qql: Qql, tmp: Path) -> None:
    say("6/7 VERIFY target + tenant isolation")
    if qql.count(DST) != POINTS:
        raise Fail("dst count mismatch")
    ok("COUNT dst = 8000")
    if facet_map(qql, SRC) != facet_map(qql, DST):
        raise Fail("FACET parity src != dst")
    ok("FACET parity src == dst")
    keys = qql.exec(f"SHOW SHARD KEYS ON COLLECTION {DST};")["results"][0]["data"][
        "shard_keys"
    ]
    if len(keys) != 12:
        raise Fail(f"want 12 shard keys, got {len(keys)}")
    ok("12 shard keys: " + ", ".join(sorted(str(k) for k in keys)))
    if qql.count(DST, shard="Mitte") != 621:
        raise Fail("SHARD Mitte count drifted (want 621)")
    ok("COUNT SHARD 'Mitte' = 621")
    vecs = write_query_vectors(tmp)
    qql.exec(f"SCROLL FROM {DST} SHARD 'Mitte' LIMIT 3;")
    qql.exec(f"FACET district FROM {DST} SHARD 'Mitte' LIMIT 5 EXACT true;")
    qql.exec(f"QUERY :dv FROM {DST} USING dense SHARD 'Spandau' LIMIT 3;", str(vecs["dense"]))
    # Writes to custom-sharded collections must carry SHARD (Qdrant requirement).
    qql.exec(
        f"UPDATE {DST} SET PAYLOAD = {{'rating': 4.5}} "
        f"WHERE district = 'Mitte' SHARD 'Mitte' WAIT true;",
        quiet=True,
    )
    if qql.count(DST, "rating = 4.5") < 621:
        raise Fail("post-UPDATE float-equality COUNT failed")
    ok("UPDATE … SHARD 'Mitte' ok (float = filter works)")
    src_top = [
        h["id"]
        for h in qql.exec(f"QUERY :dv FROM {SRC} USING dense LIMIT 5;", str(vecs["dense"]))[
            "results"
        ][0]["data"]
    ]
    dst_top = [
        h["id"]
        for h in qql.exec(f"QUERY :dv FROM {DST} USING dense LIMIT 5;", str(vecs["dense"]))[
            "results"
        ][0]["data"]
    ]
    if src_top != dst_top:
        raise Fail(f"top-k drift src={src_top} dst={dst_top}")
    ok(f"top-5 parity {src_top}")


def dump_round_trip(qql: Qql, batch: int, tmp: Path) -> None:
    say("7/7 DUMP sharded target + replay into a fresh collection")
    dump_file = tmp / "dst.qql"
    qql.run("dump", DST, str(dump_file), "--batch-size", str(batch), live=True)
    script = dump_file.read_text()
    if script.count("CREATE SHARD KEY") != 12 or "SHARD '" not in script:
        raise Fail("dump must emit CREATE SHARD KEY + SHARD-routed UPSERTs")
    replay_file = tmp / "rst.qql"
    replay_file.write_text(script.replace(DST, RST))
    qql.run("exec", f"DROP COLLECTION {RST};", check=False)
    print("  replaying dump script (~30s, output parsed at the end)…", flush=True)
    proc = qql.run("execute", "--stop-on-error", str(replay_file))
    summary = json.loads(proc.stdout)
    if summary.get("failed"):
        raise Fail(f"replay failed: {proc.stdout}")
    ok(f"replay ok ({summary.get('succeeded')} statements, 0 failed)")
    if qql.count(RST) != POINTS:
        raise Fail("restored count mismatch")
    if facet_map(qql, DST) != facet_map(qql, RST):
        raise Fail("FACET parity dst != rst")
    ok("round-trip parity dst == rst (COUNT + FACET)")
    qql.run("exec", f"DROP COLLECTION {RST};", check=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--url", default="http://localhost:6333")
    parser.add_argument("--qql", default=None)
    parser.add_argument("--batch", type=int, default=128)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--skip-ingest", action="store_true")
    parser.add_argument("--skip-migrate", action="store_true")
    parser.add_argument("--clean", action="store_true")
    args = parser.parse_args()

    try:
        qql = Qql(newest_qql(args.qql), args.url)
        if args.clean:
            say(f"DROP {SRC} and {DST}")
            qql.run("exec", f"DROP COLLECTION {SRC};", check=False)
            qql.run("exec", f"DROP COLLECTION {DST};", check=False)
            print("cleaned.")
            return 0
        with tempfile.TemporaryDirectory(prefix="qql-berlin-") as td:
            tmp = Path(td)
            check_prereqs(qql)
            if not args.skip_ingest:
                create_source(qql)
                ingest(qql, args.batch, tmp)
            verify_source(qql, tmp)
            if args.skip_migrate:
                ok("done (ingest+verify only, --skip-migrate)")
                return 0
            dry_run(qql)
            migrate(qql, args.batch, args.workers)
            verify_target(qql, tmp)
            dump_round_trip(qql, 500, tmp)
        print()
        ok(f"MIGRATION COMPLETE: {SRC} (auto) -> {DST} (custom, 12 shard keys, turbo-1.5)")
        return 0
    except Fail as err:
        print(f"FAIL {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
