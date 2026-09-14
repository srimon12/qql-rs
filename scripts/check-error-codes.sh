#!/usr/bin/env bash
#
# check-error-codes.sh — CI gate: the website error-code reference must match
# the codes the implementation actually emits.
#
# - Scans SCAN_ROOTS for `QQL-*` literals (same extraction the reference page
#   documents in its "Complete code set" section).
# - Fails on emitted-but-undocumented codes, documented-but-unemitted codes,
#   emitted codes missing a per-section table row, and an unsorted set block.
#
# Curated exceptions live in ARTIFACTS / ALLOWLIST below with the reason
# attached. Extend those lists — never weaken the checks.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REF="$ROOT/website/src/content/docs/docs/reference/error-codes.mdoc"

# Same roots the reference page's "Complete code set" is generated from.
SCAN_ROOTS=(
  crates/qql-core/src
  crates/qql-plan/src
  crates/qql-edge/src
  crates/qql-embed/src
  crates/qql-runtime/src
)

# Matches that are family-prefix usages (starts_with / test predicates), never
# emitted as a full `code` value. `QQL-PARSE` is asserted via `starts_with` in
# runtime batch tests; trailing-dash hits are `QQL-X-*` doc wildcards.
ARTIFACTS=(
  'QQL-PARSE'
)

# Codes emitted outside SCAN_ROOTS. Each entry is verified below to still be
# emitted somewhere in the workspace, so this list cannot rot silently.
# - QQL-BIND-ALREADY-BOUND: pyqql-common, nqql-common, qql-wasm statement bind.
ALLOWLIST=(
  'QQL-BIND-ALREADY-BOUND'
)

fail=0
report() { printf 'error-codes: %s\n' "$1"; fail=1; }

cd "$ROOT"

extract() {
  # Test doubles are excluded: they re-emit codes (or invented ones like the
  # mock's bare `QQL-EXECUTION`) that are not part of the shipped contract.
  # File selection uses find (not tool globs) so rg and grep behave identically.
  if command -v rg >/dev/null 2>&1; then
    find "${SCAN_ROOTS[@]}" -type f -name '*.rs' -not -path '*test*' \
      -exec rg -o --no-filename 'QQL-[A-Z0-9-]+' {} +
  else
    find "${SCAN_ROOTS[@]}" -type f -name '*.rs' -not -path '*test*' \
      -exec grep -hoE 'QQL-[A-Z0-9-]+' {} +
  fi
}

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

extract | LC_ALL=C sort -u >"$TMPDIR/emitted_raw.txt"
# Drop truncation artifacts (doc wildcards like `QQL-TRANSPORT-*`).
grep -v -e '-$' "$TMPDIR/emitted_raw.txt" >"$TMPDIR/emitted_no_dash.txt" || true
cp "$TMPDIR/emitted_no_dash.txt" "$TMPDIR/emitted.txt"
for artifact in "${ARTIFACTS[@]}"; do
  grep -v -x -F "$artifact" "$TMPDIR/emitted.txt" >"$TMPDIR/next.txt" || true
  mv "$TMPDIR/next.txt" "$TMPDIR/emitted.txt"
done

# The ```text fence pair under "## Complete code set".
awk '
  /^## Complete code set/ { in_section = 1 }
  in_section && /^```text$/ { in_block = 1; next }
  in_block && /^```$/ { exit }
  in_block { print }
' "$REF" >"$TMPDIR/documented_raw.txt"
LC_ALL=C sort -u "$TMPDIR/documented_raw.txt" >"$TMPDIR/documented.txt"

# A. Emitted but not in the documented set.
while IFS= read -r code; do
  [ -z "$code" ] && continue
  report "emitted but undocumented: $code (add a table row + set entry in $REF)"
done < <(comm -23 "$TMPDIR/emitted.txt" "$TMPDIR/documented.txt")

# B. Documented but no longer emitted (and not allowlisted).
while IFS= read -r code; do
  [ -z "$code" ] && continue
  allowed=0
  for item in "${ALLOWLIST[@]}"; do
    if [ "$code" = "$item" ]; then allowed=1; break; fi
  done
  if [ "$allowed" -eq 0 ]; then
    report "documented but not emitted: $code (remove from $REF, or fix the code)"
  fi
done < <(comm -13 "$TMPDIR/emitted.txt" "$TMPDIR/documented.txt")

# C. Allowlist entries must still be emitted somewhere in the workspace.
for code in "${ALLOWLIST[@]}"; do
  if ! grep -rqF --include='*.rs' --exclude-dir=target "$code" "$ROOT/crates" 2>/dev/null; then
    report "stale allowlist entry: $code is not emitted anywhere in crates/"
  fi
done

# D. Every emitted code (and every allowlisted one) needs a per-section table row.
while IFS= read -r code; do
  [ -z "$code" ] && continue
  if ! grep -qF "\`$code\`" "$REF"; then
    report "missing table row for emitted code: $code"
  fi
done < <(cat "$TMPDIR/emitted.txt"; printf '%s\n' "${ALLOWLIST[@]}")

# E. The set block stays sorted so diffs review cleanly (checked on raw order).
if ! LC_ALL=C sort -c "$TMPDIR/documented_raw.txt" 2>/dev/null; then
  report "complete code set block is not LC_ALL=C sorted"
fi

if [ "$fail" -eq 0 ]; then
  count="$(wc -l <"$TMPDIR/emitted.txt" | tr -d ' ')"
  printf 'error-codes: OK (%s emitted codes all documented with table rows)\n' "$count"
fi
exit "$fail"
