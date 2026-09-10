"""Shared configuration for the QQL Edge vs Qdrant Edge head-to-head.

Both contenders run the *same* qdrant-edge core (0.8.0) in-process:

  official  qdrant-edge-py 0.8.0   (Qdrant's own Python bindings)
  qql       pyqql-edge 0.4.0       (QQL language + planner + FastEmbed on
                                    top of the qdrant-edge crate)

Corpus + precomputed vectors are re-used byte-identically from the remote
`vs-qdrant` benchmark, so this folder only measures the in-process layer.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parent
VSQ = ROOT.parent / "vs-qdrant"
DATA = VSQ / "data"
WORK = ROOT / "work"
RESULTS = ROOT / "results"

# Path to the pyqql-edge Python package (built abi3 extension).
PYQQ_EDGE_PKG = ROOT.parent / "crates" / "pyqql-edge"

# Scenario parameters (mirroring vs-qdrant/common/bench.json).
LIMIT = 10
SCROLL_PAGES = 3
SCROLL_BATCH = 256
BATCH_BERLIN = 100
BATCH_LEGAL = 32
POINTS = {"berlin": 8000, "legal": 2000}

DENSE_DIM = 384
COLBERT_DIM = 128
MODEL = "bge-small-en-v1.5"  # FASTEmbed model name (pyqql-edge only)
HF_MODEL = "BAAI/bge-small-en-v1.5"  # full name for fastembed Python

# Timing defaults. In-process calls are microseconds, so iters is larger
# than the remote benchmark's.
READS_REPS = 5
READS_ITERS = 50
PREPARED_ITERS = 100
COLBERT_REPS = 3
COLBERT_ITERS = 10

COLLECTIONS = {
    "berlin": {"dense": 384, "sparse": "bm25"},
    "legal": {"dense": 384, "sparse": "bm25", "colbert": 128},
}
