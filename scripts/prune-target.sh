#!/usr/bin/env bash
# Selective target hygiene — NOT a full `cargo clean`.
#
# Frees stale build artifacts while keeping currently linked deps/rlibs so the
# next compile is not a from-scratch world rebuild.
#
# Usage:
#   scripts/prune-target.sh              # incremental + cargo-sweep (14d)
#   scripts/prune-target.sh --deep       # also drop unused profiles / old deps (7d)
#   scripts/prune-target.sh --dry-run    # print sizes only
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DAYS="${PRUNE_DAYS:-14}"
DEEP=0
DRY=0

for arg in "$@"; do
  case "$arg" in
    --deep) DEEP=1; DAYS=7 ;;
    --dry-run|-n) DRY=1 ;;
    --days=*) DAYS="${arg#--days=}" ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
  esac
done

export PATH="${HOME}/.cargo/bin:${PATH}"

echo "== target footprint =="
du -sh \
  "$ROOT/target" \
  "$ROOT/bench/bench_rust/target" \
  "$ROOT"/examples/rust/*/target \
  2>/dev/null || true

if [[ "$DRY" -eq 1 ]]; then
  echo "(dry-run) would prune incremental + cargo-sweep -t ${DAYS}"
  exit 0
fi

# 1) Incremental caches bloat forever (many hash variants of the same crate).
#    Safe to drop: rustc/deps stay; next build regenerates only what you touch.
echo
echo "== removing incremental caches =="
while IFS= read -r -d '' d; do
  sz=$(du -sh "$d" 2>/dev/null | awk '{print $1}')
  echo "  rm -rf $d ($sz)"
  rm -rf "$d"
done < <(find "$ROOT" -type d -name incremental -path '*/target/*' -print0 2>/dev/null)

# 2) Drop older hash variants in deps/ (same crate rebuilt many times leaves
#    multi‑GB stale rlibs, e.g. 11× libqdrant_edge ≈ 15G). Keep the newest.
echo
echo "== pruning stale hash variants in deps/ =="
python3 - "$ROOT" <<'PY'
import re, sys
from collections import defaultdict
from pathlib import Path

root = Path(sys.argv[1])
pat = re.compile(r"^(lib)?(.+)-([0-9a-f]{16})(\..+)?$")
# Heavy crates: keep only the newest artifact per (name, extension).
HEAVY_PREFIXES = ("qdrant_edge", "qql", "fastembed", "ort", "tokenizers", "candle")

removed = freed = 0
for deps in root.glob("**/target/*/deps"):
    if not deps.is_dir():
        continue
    groups = defaultdict(list)
    for p in deps.iterdir():
        if not p.is_file():
            continue
        m = pat.match(p.name)
        if not m:
            continue
        prefix, name, _h, ext = m.group(1) or "", m.group(2), m.group(3), m.group(4) or ""
        groups[(prefix, name, ext)].append(p)

    for (prefix, name, ext), files in groups.items():
        files.sort(key=lambda p: p.stat().st_mtime, reverse=True)
        heavy = name.startswith(HEAVY_PREFIXES) or name.startswith("qql_")
        max_keep = 1 if heavy else 2
        for p in files[max_keep:]:
            try:
                sz = p.stat().st_size
                p.unlink()
                removed += 1
                freed += sz
            except OSError:
                pass

print(f"  removed {removed} stale artifacts (~{freed / 1024**3:.1f} GiB)")
PY

# 3) cargo-sweep: drop artifacts whose fingerprints weren't used recently.
if command -v cargo-sweep >/dev/null 2>&1; then
  echo
  echo "== cargo sweep (not accessed in ${DAYS}d) =="
  for manifest_dir in "$ROOT" "$ROOT/bench/bench_rust"; do
    if [[ -f "$manifest_dir/Cargo.toml" ]]; then
      echo "  -> $manifest_dir"
      (cd "$manifest_dir" && cargo sweep -t "$DAYS" 2>/dev/null) || true
    fi
  done
else
  echo
  echo "(skip cargo-sweep — not installed: cargo install cargo-sweep)"
fi

# 4) Optional deeper trim: secondary profiles you rarely need daily.
if [[ "$DEEP" -eq 1 ]]; then
  echo
  echo "== deep: optional profile dirs (rebuilt only if you use them) =="
  for d in \
    "$ROOT/target/doc" \
    "$ROOT/target/wasm32-unknown-unknown" \
    "$ROOT/target/x86_64-unknown-linux-gnu" \
    "$ROOT/target/flycheck0"
  do
    if [[ -d "$d" ]]; then
      sz=$(du -sh "$d" | awk '{print $1}')
      echo "  rm -rf $d ($sz)"
      rm -rf "$d"
    fi
  done
fi

# 5) sccache stats if present
if command -v sccache >/dev/null 2>&1; then
  echo
  echo "== sccache =="
  sccache --show-stats 2>/dev/null || true
fi

echo
echo "== after =="
du -sh "$ROOT/target" 2>/dev/null || true
echo "done."
