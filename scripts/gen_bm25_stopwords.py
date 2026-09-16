#!/usr/bin/env python3
"""Port Qdrant's per-language stopword lists into a qql-embed Rust module.

Reads lib/segment/.../stop_words/<lang>.rs (`pub const X_STOPWORDS: &[&str]`)
and emits 30 `phf::Set` statics plus a Language-keyed lookup. Re-run to
re-sync; output is deterministic (sorted, deduped).

Qdrant is Apache-2.0; the emitted module carries the attribution header.
Usage: python3 gen_stopwords.py /data/codebases/qdrant <out.rs>
"""

import re
import sys
from pathlib import Path

LANGUAGES = [
    "arabic", "azerbaijani", "basque", "bengali", "catalan", "chinese",
    "danish", "dutch", "english", "finnish", "french", "german", "greek",
    "hebrew", "hinglish", "hungarian", "indonesian", "italian", "japanese",
    "kazakh", "nepali", "norwegian", "portuguese", "romanian", "russian",
    "slovene", "spanish", "swedish", "tajik", "turkish",
]

VARIANT = {lang: "".join(p.capitalize() for p in lang.split("_")) for lang in LANGUAGES}


def parse_list(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    body = text.split("= &", 1)[1]
    words = re.findall(r'"((?:[^"\\]|\\.)*)"', body)
    out = []
    for w in words:
        # Source files are literal UTF-8; only unescape when the raw text
        # actually contains a backslash escape (e.g. `\"`).
        if "\\" in w:
            w = w.encode("utf-8").decode("unicode_escape").encode("latin-1").decode("utf-8")
        out.append(w)
    # Deterministic, deduped (phf_set! rejects duplicate keys).
    return sorted(set(out))


def rust_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def main() -> None:
    qdrant, out_path = Path(sys.argv[1]), Path(sys.argv[2])
    src = qdrant / "lib/segment/src/index/field_index/full_text_index/stop_words"

    sets = {lang: parse_list(src / f"{lang}.rs") for lang in LANGUAGES}
    total = sum(len(v) for v in sets.values())
    print(f"languages={len(sets)} words={total}", file=sys.stderr)

    lines = [
        "//! Per-language stopword lists, ported from Qdrant's",
        "//! `lib/segment/src/index/field_index/full_text_index/stop_words/`.",
        "//!",
        "//! Attribution: (c) Qdrant contributors, Apache-2.0",
        "//! (<https://github.com/qdrant/qdrant/blob/master/LICENSE>). Word lists are",
        "//! emitted verbatim (sorted, deduped — `phf_set!` rejects duplicate keys);",
        "//! empty upstream lists stay empty. Re-sync with `scripts/gen_bm25_stopwords.py`.",
        "//!",
        "//! GENERATED — do not hand-edit; re-run the generator instead.",
        "",
        "use phf::phf_set;",
        "",
        "use super::bm25_lang::Language;",
        "",
    ]
    for lang in LANGUAGES:
        words = sets[lang]
        lines.append(f"static {lang.upper()}: phf::Set<&'static str> = phf_set! {{")
        for w in words:
            lines.append(f'    "{rust_escape(w)}",')
        lines.append("};")
        lines.append("")
    lines.append("/// Qdrant's stopword list for `language` (entries as upstream ships them).")
    lines.append("pub(super) fn stopwords_for(language: Language) -> &'static phf::Set<&'static str> {")
    lines.append("    match language {")
    for lang in LANGUAGES:
        lines.append(f"        Language::{VARIANT[lang]} => &{lang.upper()},")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    out_path.write_text("\n".join(lines), encoding="utf-8")


if __name__ == "__main__":
    main()
