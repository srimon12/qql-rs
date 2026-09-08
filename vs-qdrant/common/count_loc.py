#!/usr/bin/env python3
"""Count the application LOC each contender needs for the identical scenario
set. Counts non-blank, non-comment lines in each language's two scenario
files (harness/timing/parity code is deliberately excluded — these files
contain only the scenario implementations).
"""
from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def py_loc(p: Path) -> tuple[int, int]:
    src = p.read_text()
    total = len([l for l in src.splitlines() if l.strip()])
    code = 0
    for line in src.splitlines():
        t = line.strip()
        if not t or t.startswith("#"):
            continue
        code += 1
    return code, total

def js_loc(p: Path) -> tuple[int, int]:
    code = 0
    src = p.read_text()
    for line in src.splitlines():
        t = line.strip()
        if not t or t.startswith("//") or t.startswith("/*") or t.startswith("*"):
            continue
        code += 1
    return code, len([l for l in src.splitlines() if l.strip()])

def rs_loc(p: Path) -> tuple[int, int]:
    code = 0
    src = p.read_text()
    for line in src.splitlines():
        t = line.strip()
        if not t or t.startswith("//"):
            continue
        code += 1
    return code, len([l for l in src.splitlines() if l.strip()])

def main() -> None:
    out = {}
    for lang, ext, counter in (("python", "py", py_loc), ("node", "js", js_loc), ("rust", "rs", rs_loc)):
        d = ROOT / lang
        files = {
            "official": d / ("src/official.rs" if lang == "rust" else f"official_scenarios.{ext}"),
            "qql": d / ("src/qql_side.rs" if lang == "rust" else f"qql_scenarios.{ext}"),
        }
        out[lang] = {}
        for side, path in files.items():
            code, total = counter(path)
            out[lang][side] = {"code_lines": code, "total_lines": path.read_text().count("\n") + 1}
        q, o = out[lang]["qql"]["code_lines"], out[lang]["official"]["code_lines"]
        out[lang]["ratio"] = round(q / o, 2) if o else None
    (ROOT / "results" / "loc.json").write_text(json.dumps(out, indent=2))
    print(json.dumps(out, indent=2))

if __name__ == "__main__":
    main()
