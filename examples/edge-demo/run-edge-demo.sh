#!/bin/bash
# ── QQL Edge Demo ────────────────────────────────────────────────────────────
# Runs a fully in-process vector search demo using qdrant-edge.
# No Qdrant server is needed.
#
# Embedder options (set via env):
#   EMBEDDER=fastembed  (default) — local ONNX via fastembed-rs
#   EMBEDDER=http       — external OpenAI-compatible endpoint
#
# When EMBEDDER=http:
#   EMBED_URL=http://localhost:11434/v1/embeddings  (Ollama example)
#   EMBED_KEY=                                       (empty = no auth)
#   EMBED_MODEL=nomic-embed-text
#   EMBED_DIM=768
#
# Examples:
#   bash run-edge-demo.sh                          # fastembed (fully offline)
#   EMBEDDER=http EMBED_URL=http://... bash run-edge-demo.sh
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

QQL_BIN="${QQL_BIN:-/data/codebases/qql-rs/target/debug/qql}"
EMBEDDER="${EMBEDDER:-fastembed}"
EMBED_URL="${EMBED_URL:-}"
EMBED_KEY="${EMBED_KEY:-}"
EMBED_MODEL="${EMBED_MODEL:-nomic-embed-text}"
EMBED_DIM="${EMBED_DIM:-768}"
DATA_DIR="${EDGE_DATA_DIR:-/tmp/qql-edge-demo}"

COL="edge_docs"

DEMO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARTIFACTS="$DEMO_ROOT/artifacts"
mkdir -p "$ARTIFACTS"

# ── helpers ───────────────────────────────────────────────────────────────────

edge() {
    local stmt="$1"
    local extra_flags=()

    extra_flags+=(--data-dir "$DATA_DIR" --embedder "$EMBEDDER")

    if [ "$EMBEDDER" = "http" ]; then
        extra_flags+=(
            --embed-url "$EMBED_URL"
            --embed-key "$EMBED_KEY"
            --embed-model "$EMBED_MODEL"
            --embed-dim "$EMBED_DIM"
        )
    fi

    "$QQL_BIN" edge "$stmt" "${extra_flags[@]}" --json
}

step() {
    local id="$1"
    local stmt="$2"
    local allow_fail="${3:-false}"
    local artifact="$ARTIFACTS/$id.json"
    echo -n "  [$id] ... "
    edge "$stmt" > "$artifact" 2>/dev/null || true
    if jq -e '.ok == true' "$artifact" > /dev/null 2>&1; then
        echo "✓"
    elif [ "$allow_fail" = "true" ]; then
        echo "(skip)"
    else
        echo "✗"
        jq . "$artifact" >&2
        exit 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────

echo ""
echo "══════════════════════════════════════════════════════"
echo "  QQL Edge Demo"
echo "  Embedder : $EMBEDDER"
echo "  Data dir : $DATA_DIR"
echo "══════════════════════════════════════════════════════"
echo ""

# ── 1. Provision ──────────────────────────────────────────────────────────────

echo "[1] Provision"
step "01-drop"      "DROP COLLECTION $COL" true
step "01-create"    "CREATE COLLECTION $COL HYBRID"
step "01-idx-tag"   "CREATE INDEX ON COLLECTION $COL FOR tag TYPE keyword"
step "01-idx-year"  "CREATE INDEX ON COLLECTION $COL FOR year TYPE integer"

# ── 2. Seed ───────────────────────────────────────────────────────────────────

echo "[2] Seed"
step "02-upsert-1" "UPSERT INTO $COL VALUES {
    'id': 1, 'text': 'Qdrant is a high-performance vector database for AI applications.',
    'tag': 'database', 'year': 2024
}"

step "02-upsert-2" "UPSERT INTO $COL VALUES {
    'id': 2, 'text': 'Rust achieves memory safety without a garbage collector.',
    'tag': 'systems', 'year': 2023
}"

step "02-upsert-3" "UPSERT INTO $COL VALUES {
    'id': 3, 'text': 'Hybrid search combines dense and sparse retrieval for better recall.',
    'tag': 'search', 'year': 2024
}"

step "02-upsert-4" "UPSERT INTO $COL VALUES {
    'id': 4, 'text': 'HNSW is the graph index algorithm used by qdrant-edge.',
    'tag': 'database', 'year': 2024
}"

step "02-upsert-5" "UPSERT INTO $COL VALUES {
    'id': 5, 'text': 'Sparse embeddings using BM25 complement dense semantic search.',
    'tag': 'search', 'year': 2023
}"

# ── 3. Search Modes ───────────────────────────────────────────────────────────

echo "[3] Search modes"
step "03-dense"         "QUERY 'vector similarity search' FROM $COL USING dense LIMIT 3"
step "03-sparse"        "QUERY 'vector similarity search' FROM $COL USING sparse LIMIT 3"
step "03-hybrid-rrf"    "QUERY HYBRID TEXT 'vector similarity search' DENSE dense SPARSE sparse FUSION RRF FROM $COL LIMIT 3"
step "03-hybrid-dbsf"   "QUERY HYBRID TEXT 'vector similarity search' DENSE dense SPARSE sparse FUSION DBSF FROM $COL LIMIT 3"
step "03-exact"         "QUERY 'vector similarity search' FROM $COL USING dense PARAMS (exact = true) LIMIT 3"

# ── 4. Filters ────────────────────────────────────────────────────────────────

echo "[4] Filters"
step "04-keyword"   "QUERY 'index structure' FROM $COL USING dense WHERE tag = 'database' LIMIT 3"
step "04-year"      "QUERY 'search retrieval' FROM $COL USING dense WHERE year = 2024 LIMIT 5"

# ── 5. Advanced ───────────────────────────────────────────────────────────────

echo "[5] Advanced"
step "05-threshold"     "QUERY 'high performance' FROM $COL USING dense SCORE THRESHOLD 0.0 LIMIT 5"
step "05-offset"        "QUERY 'vector database' FROM $COL USING dense LIMIT 3 OFFSET 1"
step "05-recommend"     "QUERY RECOMMEND POSITIVE (1) STRATEGY average_vector FROM $COL USING dense LIMIT 3"
step "05-prefetch"      "WITH a AS (QUERY 'vector database' FROM $COL USING dense LIMIT 10), b AS (QUERY 'vector database' FROM $COL USING sparse LIMIT 10) QUERY FUSION RRF FROM $COL PREFETCH (a, b) LIMIT 3"

# ── 6. Point Access ───────────────────────────────────────────────────────────

echo "[6] Point access"
step "06-select"    "QUERY POINTS (1) FROM $COL WITH PAYLOAD true"
step "06-scroll"    "SCROLL FROM $COL LIMIT 5"
step "06-scroll-f"  "SCROLL FROM $COL WHERE tag = 'search' LIMIT 5"

# ── 7. Mutations ─────────────────────────────────────────────────────────────

echo "[7] Mutations"
step "07-update"    "UPDATE $COL SET PAYLOAD = {year: 2025} WHERE id = 2"
step "07-delete"    "DELETE FROM $COL WHERE tag = 'systems'"

# ── 8. Inspect ────────────────────────────────────────────────────────────────

echo "[8] Inspect"
step "08-show-col"  "SHOW COLLECTION $COL"
step "08-show-all"  "SHOW COLLECTIONS"

echo ""
echo "══════════════════════════════════════════════════════"
echo "  All steps passed. Artifacts: $ARTIFACTS"
echo "══════════════════════════════════════════════════════"
echo ""

# ── Summary stats ─────────────────────────────────────────────────────────────

echo "Collection info:"
jq '{points: .data.points_count, topology: .data.topology, quantization: .data.quantization}' \
    "$ARTIFACTS/08-show-col.json"

echo ""
echo "Dense top-3 results for 'vector similarity search':"
jq '[.data[] | {id, score: (.score | floor * 1000 / 1000)}]' \
    "$ARTIFACTS/03-dense.json" 2>/dev/null || true
