#!/bin/bash

set -euo pipefail

EVAL_PATH="${1:?eval path required}"
ARTIFACTS="${2:?artifact dir required}"

if ! command -v jq >/dev/null 2>&1; then
    echo "validate-artifacts.sh requires jq" >&2
    exit 1
fi

assert_jq() {
    local file="$1"
    local expr="$2"
    local message="$3"
    if ! jq -e "$expr" "$file" >/dev/null; then
        echo "Assertion failed: $message" >&2
        echo "  file: $file" >&2
        exit 1
    fi
}

MAIN_ID="$(jq -r '.queries.main.id' "$EVAL_PATH")"
MAIN_SPECIALTY="$(jq -r '.queries.main.specialty' "$EVAL_PATH")"
MAIN_TENANT="$(jq -r '.queries.main.tenant_id' "$EVAL_PATH")"
MAIN_PRIORITY="$(jq -r '.queries.main.case_priority' "$EVAL_PATH")"
MAIN_STATUS="$(jq -r '.queries.main.case_status' "$EVAL_PATH")"
MIN_ROWS="$(jq -r '.row_count' "$EVAL_PATH")"
MAIN_QUESTION="$(jq -r '.queries.main.question' "$EVAL_PATH")"

assert_jq "$ARTIFACTS/01-doctor.json" '.ok == true and .healthy == true' "doctor should report a healthy connection"
assert_jq "$ARTIFACTS/04-inspect.json" ".ok == true and .data.topology == \"hybrid\" and (.data.points_count // 0) >= $MIN_ROWS and .data.quantization == \"scalar\"" "collection should be hybrid, quantized, and contain the generated rows"
assert_jq "$ARTIFACTS/04-inspect.json" '.data.payload_schema.case_priority.type == "keyword" and .data.payload_schema.topic_text.type == "text"' "required payload indexes should exist"
assert_jq "$ARTIFACTS/05-explain-hybrid-rrf.json" '.ok == true and (.plan | contains("QUERY NEAREST"))' "hybrid explain plan should be present"
assert_jq "$ARTIFACTS/06-search-dense.json" ".ok == true and ([.data[].id] | index(\"$MAIN_ID\")) != null" "main document should appear in dense results"
assert_jq "$ARTIFACTS/07-search-sparse.json" '.ok == true and (.data | length) >= 1' "sparse search should return medical matches"
assert_jq "$ARTIFACTS/08-search-hybrid-rrf.json" ".ok == true and ([.data[].id] | index(\"$MAIN_ID\")) != null" "main document should appear in hybrid RRF results"
assert_jq "$ARTIFACTS/09-search-hybrid-dbsf.json" ".ok == true and ([.data[].id] | index(\"$MAIN_ID\")) != null" "main document should appear in hybrid DBSF results"
assert_jq "$ARTIFACTS/10-search-exact.json" ".ok == true and ([.data[].id] | index(\"$MAIN_ID\")) != null" "main document should appear in exact results"
assert_jq "$ARTIFACTS/11-search-filtered-tenant.json" ".ok == true and (.data | length) >= 1 and ([.data[].id] | index(\"$MAIN_ID\")) != null" "tenant-filtered active high-priority search should keep the main document"
assert_jq "$ARTIFACTS/12-search-score-threshold.json" ".ok == true and (.data | length) >= 1" "score-thresholded hybrid search should return medical matches"
assert_jq "$ARTIFACTS/13-search-offset-window.json" ".ok == true and (.data | length) >= 1" "offset hybrid search should return the next result window"
assert_jq "$ARTIFACTS/14-search-grouped-specialty.json" ".ok == true and ([.data[].group_id] | index(\"$MAIN_SPECIALTY\")) != null" "grouped search should surface the specialty groups"
assert_jq "$ARTIFACTS/15-search-hybrid-mmr.json" '.ok == true and (.data | length) >= 1' "hybrid MMR search should return diversified medical matches"
assert_jq "$ARTIFACTS/16-select-main.json" '.ok == true and .data.payload.tenant_id == "'"$MAIN_TENANT"'" and .data.payload.case_priority == "'"$MAIN_PRIORITY"'" and .data.payload.case_status == "'"$MAIN_STATUS"'"' "selected document should preserve tenant and case metadata"
assert_jq "$ARTIFACTS/17-recommend-related.json" '.ok == true and (.data | length) >= 1' "recommend should return related medical answers"
assert_jq "$ARTIFACTS/18-scroll-tenant.json" ".ok == true and (.data.points | length) >= 1 and ([.data.points[].payload.tenant_id] | all(. == \"$MAIN_TENANT\"))" "tenant scroll should stay inside one tenant"
assert_jq "$ARTIFACTS/19-dump.json" '.ok == true' "dump should succeed"
assert_jq "$ARTIFACTS/20-benchmark.json" '.modes | length >= 5' "benchmark should report all retrieval modes"

echo "Validated medical retrieval artifacts for question '$MAIN_QUESTION' in $ARTIFACTS"
