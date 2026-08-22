#!/usr/bin/env python3
"""Cross-check vendored Qdrant API surfaces against upstream Qdrant.

The protocol surfaces under ``crates/qql-runtime`` are vendored from the
Qdrant repository and must stay in lockstep with it:

- ``openapi.json``            REST schema (verbatim copy)
- ``proto/*.proto``           public gRPC protos (verbatim copies)
- ``proto/qdrant.proto``      umbrella proto, derived from upstream by
                              stripping imports of protos we do not vendor
                              (internal cluster services)
- ``proto/quota_internal.proto``
                              internal messages-only proto, vendored verbatim
                              for reference; NOT compiled into the runtime

Usage::

    python3 scripts/sync_qdrant_api.py --check          # CI gate
    python3 scripts/sync_qdrant_api.py --update         # refresh vendored copies
    python3 scripts/sync_qdrant_api.py --update --ref v1.20.0

``--update`` resolves the ref to an immutable commit SHA and records it in
``scripts/qdrant-api-manifest.json`` together with the SHA-256 of every
upstream source file. ``--check`` re-downloads at that pinned SHA and fails
with a per-file drift report when anything diverges.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "crates" / "qql-runtime"
MANIFEST_PATH = ROOT / "scripts" / "qdrant-api-manifest.json"

UPSTREAM_REPO = "qdrant/qdrant"
PROTO_UPSTREAM_DIR = "lib/api/src/grpc/proto"
OPENAPI_UPSTREAM_PATH = "docs/redoc/master/openapi.json"

# Vendored verbatim from the public gRPC surface.
VERBATIM_PROTOS = (
    "collections.proto",
    "collections_service.proto",
    "json_with_int.proto",
    "points.proto",
    "points_service.proto",
    "qdrant_common.proto",
    "snapshots_service.proto",
)

# Internal, messages-only protos kept in tree for reference. They are not
# imported by our trimmed umbrella proto and never compiled.
REFERENCE_PROTOS = (
    "quota_internal.proto",
)

UMBRELLA_PROTO = "qdrant.proto"
OPENAPI_FILE = "openapi.json"

IMPORT_RE = re.compile(r'^import\s+"([^"]+)";\s*$')


def http_get(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "qql-rs-sync"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def resolve_commit(ref: str) -> str:
    url = f"https://api.github.com/repos/{UPSTREAM_REPO}/commits/{ref}"
    payload = json.loads(http_get(url))
    return payload["sha"]


def upstream_url(commit: str, path: str) -> str:
    return (
        f"https://raw.githubusercontent.com/{UPSTREAM_REPO}/{commit}/{path}"
    )


def fetch_upstream(commit: str) -> dict[str, bytes]:
    """Download every tracked upstream file at ``commit``."""
    files: dict[str, bytes] = {}
    for name in (*VERBATIM_PROTOS, *REFERENCE_PROTOS, UMBRELLA_PROTO):
        files[name] = http_get(upstream_url(commit, f"{PROTO_UPSTREAM_DIR}/{name}"))
    files[OPENAPI_FILE] = http_get(upstream_url(commit, OPENAPI_UPSTREAM_PATH))
    return files


def derive_umbrella(upstream: bytes, vendored_names: set[str]) -> bytes:
    """Strip imports of protos we do not vendor from the upstream umbrella."""
    out: list[str] = []
    for line in upstream.decode("utf-8").splitlines(keepends=True):
        match = IMPORT_RE.match(line.strip())
        if match and match.group(1).split("/")[-1] not in vendored_names:
            continue
        out.append(line)
    return "".join(out).encode("utf-8")


def load_manifest() -> dict:
    if not MANIFEST_PATH.is_file():
        fail(f"manifest {MANIFEST_PATH.relative_to(ROOT)} is missing; run --update first")
    return json.loads(MANIFEST_PATH.read_text())


def fail(message: str) -> None:
    print(f"sync error: {message}", file=sys.stderr)
    raise SystemExit(1)


def do_update(ref: str) -> None:
    commit = resolve_commit(ref)
    print(f"upstream {UPSTREAM_REPO}@{ref} -> {commit[:12]}")
    files = fetch_upstream(commit)

    vendored_names = {Path(name).name for name in VERBATIM_PROTOS}
    derived_umbrella = derive_umbrella(files[UMBRELLA_PROTO], vendored_names)

    (RUNTIME / "proto" / UMBRELLA_PROTO).write_bytes(derived_umbrella)
    for name in VERBATIM_PROTOS:
        (RUNTIME / "proto" / name).write_bytes(files[name])
    for name in REFERENCE_PROTOS:
        (RUNTIME / "proto" / name).write_bytes(files[name])
    (RUNTIME / OPENAPI_FILE).write_bytes(files[OPENAPI_FILE])

    manifest = {
        "repo": UPSTREAM_REPO,
        "ref": ref,
        "commit": commit,
        "sources": {
            name: {
                "path": (
                    OPENAPI_UPSTREAM_PATH
                    if name == OPENAPI_FILE
                    else f"{PROTO_UPSTREAM_DIR}/{name}"
                ),
                "sha256": hashlib.sha256(data).hexdigest(),
            }
            for name, data in files.items()
        },
    }
    MANIFEST_PATH.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"updated {len(files)} vendored files; manifest written")


def do_check() -> None:
    manifest = load_manifest()
    commit = manifest["commit"]
    expected_sources = manifest.get("sources", {})
    print(f"checking against {manifest['repo']}@{commit[:12]} (ref {manifest['ref']})")

    files = fetch_upstream(commit)
    vendored_names = {Path(name).name for name in VERBATIM_PROTOS}

    drifted: list[str] = []
    missing: list[str] = []

    def compare(name: str, expected: bytes) -> None:
        target = RUNTIME / OPENAPI_FILE if name == OPENAPI_FILE else RUNTIME / "proto" / name
        if not target.is_file():
            missing.append(name)
            return
        if target.read_bytes() != expected:
            drifted.append(name)

    for name in VERBATIM_PROTOS:
        compare(name, files[name])
    for name in REFERENCE_PROTOS:
        compare(name, files[name])
    compare(OPENAPI_FILE, files[OPENAPI_FILE])
    compare(UMBRELLA_PROTO, derive_umbrella(files[UMBRELLA_PROTO], vendored_names))

    # Upstream sources must also still hash to what the manifest recorded,
    # otherwise the pin itself was tampered with or GitHub served stale data.
    for name, meta in expected_sources.items():
        actual = hashlib.sha256(files[name]).hexdigest()
        if actual != meta["sha256"]:
            drifted.append(f"{name} (manifest hash mismatch)")

    if missing or drifted:
        for name in sorted(set(missing)):
            print(f"MISSING   {name}")
        for name in sorted(set(drifted)):
            print(f"DRIFTED   {name}")
        fail(
            "vendored Qdrant API surface is out of sync; "
            "run `python3 scripts/sync_qdrant_api.py --update` and review the diff"
        )
    print(
        f"in sync: {len(VERBATIM_PROTOS)} public protos, "
        f"{len(REFERENCE_PROTOS)} reference proto, derived {UMBRELLA_PROTO}, {OPENAPI_FILE}"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--check", action="store_true", help="verify vendored files match the pinned upstream commit")
    modes.add_argument("--update", action="store_true", help="re-download and overwrite vendored files")
    parser.add_argument("--ref", default=None, help="upstream ref for --update (default: master)")
    args = parser.parse_args()

    if args.update:
        do_update(args.ref or "master")
    else:
        do_check()


if __name__ == "__main__":
    main()
