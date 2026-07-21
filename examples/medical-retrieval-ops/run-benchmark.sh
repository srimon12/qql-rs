#!/bin/bash

set -euo pipefail

if ! command -v qql-go >/dev/null 2>&1; then
    echo "Error: qql-go must be installed and available on PATH" >&2
    exit 1
fi

if ! command -v uv >/dev/null 2>&1; then
    echo "Error: run-benchmark requires uv" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QUESTIONS_PATH="${1:-$SCRIPT_DIR/generated/benchmark-questions.json}"

uv run "$SCRIPT_DIR/run-benchmark.py" "$QUESTIONS_PATH"
