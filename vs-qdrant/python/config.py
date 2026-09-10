"""Shared configuration loader — single source of truth is common/bench.json."""
from __future__ import annotations

import json
from pathlib import Path

_cfg = json.loads((Path(__file__).resolve().parent.parent / "common" / "bench.json").read_text())

URL = _cfg["url"]
# gRPC endpoint of the same server (Qdrant serves :6334 next to REST :6333).
URL_GRPC = "http://localhost:6334"
LIMIT = _cfg["limit"]
SCROLL_PAGES = _cfg["scroll_pages"]
SCROLL_BATCH = _cfg["scroll_batch"]
BATCH_BERLIN = _cfg["berlin"]["batch"]
BATCH_LEGAL = _cfg["legal"]["batch"]
COLLS = {k: v["collections"] for k, v in _cfg.items() if isinstance(v, dict) and "collections" in v}
