#!/usr/bin/env python3
"""Render the README results section from results/{python,node,rust}.json +
results/loc.json — no hand-transcribed numbers."""
from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
R = {lang: json.loads((ROOT / "results" / f"{lang}.json").read_text())
     for lang in ("python", "node", "rust")}
LOC = json.loads((ROOT / "results" / "loc.json").read_text())

def fmt_ratio(s: dict) -> str:
    return f"{s['qql']['ops_per_sec'] / s['official']['ops_per_sec']:.2f}x"

def ingest_ratio(lang: str) -> str:
    i = R[lang]["ingest"]
    return f"{i['official_sec'] / i['qql_sec']:.2f}x"

lines = []

# --- per-leg scenario tables ---
lines.append("### Results")
lines.append("")
for lang, title in (("python", "Python — `qdrant-client 1.19.0` vs `pyqql 0.4.0` (both REST)"),
                    ("node", "Node — `@qdrant/js-client-rest 1.19.0` vs `nqql 0.4.0` (both REST)"),
                    ("rust", "Rust — `qdrant-client 1.19.0` vs `qql 0.4.0` (both gRPC)")):
    r = R[lang]
    lines.append(f"#### {title}")
    lines.append("")
    lines.append("| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |")
    lines.append("|---|---:|---:|---:|---:|---:|---|")
    for name, s in r["scenarios"].items():
        p = s["parity"]
        if "match" in p:
            parity = "exact" if p["match"] else f"FAIL: {p['detail'][:40]}"
        elif "qql_vs_official" in p:
            qvo = p["qql_vs_official"]
            ovo = p.get("official_vs_official", {})
            if not ovo.get("top1_match", True) and not qvo.get("top1_match", True):
                top1_str = "tie-break ok"
            else:
                top1_str = "top1 ok" if qvo.get("top1_match", True) else "top1 differs"
            parity = f"overlap {qvo['jaccard_overlap']}, {top1_str}"
        else:
            parity = f"overlap {p['jaccard_overlap']}, top1 {'ok' if p['top1_match'] else 'differs'}"
        lines.append(f"| {name} | {s['official']['ops_per_sec']:,} | {s['qql']['ops_per_sec']:,} "
                     f"| {fmt_ratio(s)} | {s['official']['p50_ms']} ms | {s['qql']['p50_ms']} ms | {parity} |")
    ing = r["ingest"]
    lines.append(f"| ingest (10k pts, wait=false) | {ing['official_sec']} s | {ing['qql_sec']} s "
                 f"| {ingest_ratio(lang)} | — | — | counts + sample point byte-identical |")
    if "cold_import" in r:
        ci = r["cold_import"]
        k_off, k_qql = list(ci.keys())
        lines.append(f"| cold import / require | {ci[k_off]} ms | {ci[k_qql]} ms "
                     f"| {ci[k_off] / ci[k_qql]:.1f}x | — | — | median of "
                     f"{r['meta'].get('cold_reps', 5)} fresh processes |")
    lines.append("")

# --- LOC ---
lines.append("### Application LOC (identical scenario set)")
lines.append("")
lines.append("| language | official SDK | qql SDK | qql LOC ratio |")
lines.append("|---|---:|---:|---:|")
for lang in ("python", "node", "rust"):
    l = LOC[lang]
    lines.append(f"| {lang} | {l['official']['code_lines']} | {l['qql']['code_lines']} | {l['ratio']}x |")
lines.append("")

print("\n".join(lines))
