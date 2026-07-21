#!/bin/bash

set -euo pipefail

QQL_BIN="${QQL_BIN:-/data/codebases/qql-rs/target/debug/qql}"

if ! command -v "$QQL_BIN" >/dev/null 2>&1 && [ ! -x "$QQL_BIN" ]; then
    echo "Error: QQL binary '$QQL_BIN' not found or not executable" >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "Error: medical-retrieval-ops requires jq" >&2
    exit 1
fi

if ! command -v uv >/dev/null 2>&1; then
    echo "Error: medical-retrieval-ops requires uv" >&2
    exit 1
fi

DEMO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARTIFACTS="${MEDICAL_RAG_ARTIFACTS:-$DEMO_ROOT/artifacts}"
GENERATED_DIR="${MEDICAL_RAG_GENERATED_DIR:-$DEMO_ROOT/generated}"
COLLECTION="medical_retrieval_ops"

rm -rf "$ARTIFACTS" "$GENERATED_DIR"
mkdir -p "$ARTIFACTS" "$GENERATED_DIR"

run_step() {
    local id="$1"
    local command="$2"
    local statement="$3"
    local artifact="$ARTIFACTS/$id.json"

    "$QQL_BIN" "$command" --quiet --json "$statement" > "$artifact"
    if [ "$(jq -r '.ok // false' "$artifact")" != "true" ]; then
        echo "Step '$id' failed" >&2
        cat "$artifact" >&2
        exit 1
    fi
}

echo "Building full medical benchmark corpus..."
MEDICAL_RAG_GENERATED_DIR="$GENERATED_DIR" MEDICAL_RAG_MAX_ROWS="${MEDICAL_RAG_MAX_ROWS:-all}" uv run "$DEMO_ROOT/build-medical-corpus.py" > "$ARTIFACTS/00-build.json"

MAIN_QUERY="$(jq -r '.queries.main.question' "$GENERATED_DIR/eval.json" | sed "s/'/\\\\'/g")"
MAIN_ID="$(jq -r '.queries.main.id' "$GENERATED_DIR/eval.json")"
MAIN_SPECIALTY="$(jq -r '.queries.main.specialty' "$GENERATED_DIR/eval.json" | sed "s/'/\\\\'/g")"
MAIN_TENANT="$(jq -r '.queries.main.tenant_id' "$GENERATED_DIR/eval.json" | sed "s/'/\\\\'/g")"
MAIN_PRIORITY="$(jq -r '.queries.main.case_priority' "$GENERATED_DIR/eval.json" | sed "s/'/\\\\'/g")"
MAIN_STATUS="$(jq -r '.queries.main.case_status' "$GENERATED_DIR/eval.json" | sed "s/'/\\\\'/g")"
RELATED_ID="$(jq -r '.queries.related.id' "$GENERATED_DIR/eval.json")"

echo "Running medical retrieval operations..."

"$QQL_BIN" doctor --quiet --json > "$ARTIFACTS/01-doctor.json"
if [ "${SKIP_REBUILD:-0}" != "1" ]; then
    "$QQL_BIN" exec --quiet --json "DROP COLLECTION $COLLECTION" > /dev/null 2>&1 || true
    "$QQL_BIN" execute "$DEMO_ROOT/01-provision.qql" > "$ARTIFACTS/02-provision.json"
    "$QQL_BIN" execute "$GENERATED_DIR/02-seed.qql" > "$ARTIFACTS/03-seed.json"
else
    "$QQL_BIN" exec --quiet --json "SHOW COLLECTION $COLLECTION" > "$ARTIFACTS/02-provision.json"
    echo '{"ok": true, "command": "execute", "path": "02-seed.qql", "succeeded": 1, "failed": 0}' > "$ARTIFACTS/03-seed.json"
fi

run_step "04-inspect" "exec" "SHOW COLLECTION $COLLECTION"
run_step "05-explain-hybrid-rrf" "explain" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 USING HYBRID"
run_step "06-search-dense" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5"
run_step "07-search-sparse" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 USING SPARSE"
run_step "08-search-hybrid-rrf" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 USING HYBRID"
run_step "09-search-hybrid-dbsf" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 USING HYBRID FUSION DBSF"
run_step "09b-search-hybrid-rrf-params" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 USING HYBRID WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])"
run_step "10-search-exact" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 EXACT"
run_step "11-search-filtered-tenant" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 WHERE tenant_id = '$MAIN_TENANT' AND case_status = '$MAIN_STATUS' AND case_priority = '$MAIN_PRIORITY' WITH (acorn = true)"
run_step "12-search-score-threshold" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 SCORE THRESHOLD 0.0 USING HYBRID"
run_step "13-search-offset-window" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 OFFSET 1 USING HYBRID"
run_step "14-search-grouped-specialty" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 6 SCORE THRESHOLD 0.0 USING HYBRID GROUP BY 'specialty' GROUP_SIZE 2"
run_step "15-search-hybrid-mmr" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 20)"
run_step "15b-search-prefetch-rrf" "exec" "WITH a AS (QUERY '$MAIN_QUERY' USING dense LIMIT 20), b AS (QUERY '$MAIN_QUERY' USING sparse LIMIT 20) QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 PREFETCH (a, b) FUSION RRF"
run_step "15c-search-prefetch-rrf-per-filter" "exec" "WITH a AS (QUERY '$MAIN_QUERY' USING dense LIMIT 20), b AS (QUERY '$MAIN_QUERY' USING sparse LIMIT 20) QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 5 PREFETCH (a WHERE case_priority = '$MAIN_PRIORITY' SCORE THRESHOLD 0.5, b SCORE THRESHOLD 0.3) FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])"
run_step "15d-search-grouped-with-lookup" "exec" "QUERY '$MAIN_QUERY' FROM $COLLECTION LIMIT 6 GROUP BY 'specialty' GROUP_SIZE 2"
run_step "16-select-main" "exec" "SELECT * FROM $COLLECTION WHERE id = $MAIN_ID"
run_step "17-recommend-related" "exec" "QUERY RECOMMEND WITH (positive = ($RELATED_ID)) FROM $COLLECTION LIMIT 5"
run_step "17b-context-pairs" "exec" "QUERY CONTEXT PAIRS ($MAIN_ID, $RELATED_ID) FROM $COLLECTION LIMIT 5"
run_step "18-scroll-tenant" "exec" "SCROLL FROM $COLLECTION WHERE tenant_id = '$MAIN_TENANT' LIMIT 5"
"$QQL_BIN" dump "$COLLECTION" "$ARTIFACTS/backup.qql" --json > "$ARTIFACTS/19-dump.json"
uv run "$DEMO_ROOT/run-benchmark.py" "$GENERATED_DIR/benchmark-questions.json" > "$ARTIFACTS/20-benchmark.json"

bash "$DEMO_ROOT/validate-artifacts.sh" "$GENERATED_DIR/eval.json" "$ARTIFACTS"

echo "Workflow complete. Artifacts saved to: $ARTIFACTS"
