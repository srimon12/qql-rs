#!/usr/bin/env python3
"""Precompute every vector the benchmark needs — once, on the GPU — so that
all six SDKs (Python/Node/Rust, QQL and official) consume byte-identical
inputs. This decouples model inference from the SDK comparison entirely.

Models (identical to the ingested collections this benchmark is modelled on):
  dense   BAAI/bge-small-en-v1.5        384-d cosine   (TextEmbedding)
  colbert colbert-ir/colbertv2.0        128-d max_sim  (LateInteractionTextEmbedding)
  sparse  Qdrant/bm25                   murmur3 token ids (SparseTextEmbedding)

Outputs (all under ../data/):
  berlin.jsonl / legal.jsonl        corpus (gen_corpus.py)
  berlin_dense.f32 / legal_dense.f32            flat f32 [n x 384]
  berlin_bm25.json / legal_bm25.json            per-doc sparse {indices, values}
  legal_colbert.f32 + legal_colbert_lens.json   flat f32 + per-doc token counts
  queries.json                      fixed query set incl. dense/colbert/sparse query vectors
  manifest.json                     sha256 of every artifact + model/env info
"""
from __future__ import annotations

import hashlib
import json
import platform
import time
from pathlib import Path

import numpy as np
import onnxruntime as ort

DATA = Path(__file__).resolve().parent.parent / "data"

DENSE_MODEL = "BAAI/bge-small-en-v1.5"
COLBERT_MODEL = "colbert-ir/colbertv2.0"
SPARSE_MODEL = "Qdrant/bm25"
GPU_PROVIDERS = ["CUDAExecutionProvider", "CPUExecutionProvider"]

# Fixed query set — same texts drive every language pair's read scenarios.
QUERIES = {
    "berlin": [
        "quiet sunny apartment with a garden near the park",
        "modern studio with strong wifi for long term stays",
        "family friendly flat with parking and blackout curtains",
        "central renovated loft minutes from museum island",
    ],
    "legal": [
        "landlord retained the rental deposit without justification",
        "ordinary termination notice period miscalculated",
        "rent reduction for a material defect in the apartment",
        "unfair clause in a commercial lease agreement",
    ],
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def sparse_to_dict(sv) -> dict:
    return {"indices": [int(i) for i in sv.indices], "values": [float(v) for v in sv.values]}


def load_corpus(name: str) -> list[dict]:
    return [json.loads(l) for l in (DATA / f"{name}.jsonl").read_text().splitlines() if l.strip()]


def embed_collection(name: str, texts_for_dense, texts_for_sparse, colbert_texts=None) -> dict:
    from fastembed import TextEmbedding, SparseTextEmbedding

    print(f"[{name}] dense {DENSE_MODEL} on {ort.get_device()}...", flush=True)
    t0 = time.perf_counter()
    dense = np.array(list(TextEmbedding(DENSE_MODEL, providers=GPU_PROVIDERS)
                          .embed(texts_for_dense, batch_size=256)), dtype=np.float32)
    t_dense = time.perf_counter() - t0

    print(f"[{name}] sparse {SPARSE_MODEL}...", flush=True)
    t0 = time.perf_counter()
    sparse = [sparse_to_dict(sv) for sv in
              SparseTextEmbedding(SPARSE_MODEL).embed(texts_for_sparse)]
    t_sparse = time.perf_counter() - t0

    (DATA / f"{name}_dense.f32").write_bytes(dense.tobytes())
    (DATA / f"{name}_bm25.json").write_text(json.dumps(sparse))

    out = {"dense_shape": list(dense.shape), "dense_sec": round(t_dense, 1),
           "sparse_n": len(sparse), "sparse_sec": round(t_sparse, 1)}

    if colbert_texts is not None:
        from fastembed import LateInteractionTextEmbedding
        print(f"[{name}] colbert {COLBERT_MODEL}...", flush=True)
        t0 = time.perf_counter()
        lens, chunks = [], []
        # GTX-class 3 GB GPUs OOM on fastembed's default colbert batch — small
        # batches keep the VRAM envelope flat (quality is unaffected).
        for mv in LateInteractionTextEmbedding(COLBERT_MODEL, providers=GPU_PROVIDERS).embed(colbert_texts, batch_size=4):
            arr = np.asarray(mv, dtype=np.float32)
            lens.append(arr.shape[0])
            chunks.append(arr.reshape(-1))
        colbert = np.concatenate(chunks)
        t_col = time.perf_counter() - t0
        (DATA / f"{name}_colbert.f32").write_bytes(colbert.tobytes())
        (DATA / f"{name}_colbert_lens.json").write_text(json.dumps(lens))
        out |= {"colbert_shape": list(colbert.shape), "colbert_tokens_total": int(sum(lens)),
                "colbert_sec": round(t_col, 1)}
    print(f"[{name}] {out}", flush=True)
    return out


def embed_queries() -> None:
    from fastembed import (LateInteractionTextEmbedding, SparseTextEmbedding,
                           TextEmbedding)
    queries = {"berlin": [], "legal": []}
    print("[queries] dense + sparse + colbert...", flush=True)
    for domain, texts in QUERIES.items():
        dense = np.array(list(TextEmbedding(DENSE_MODEL, providers=GPU_PROVIDERS).query_embed(texts)), dtype=np.float32)
        sparse = [sparse_to_dict(sv) for sv in SparseTextEmbedding(SPARSE_MODEL).embed(texts)]
        for i, text in enumerate(texts):
            q = {"text": text, "dense": dense[i].tolist(), "sparse": sparse[i]}
            if domain == "legal":
                mv = np.asarray(list(LateInteractionTextEmbedding(
                    COLBERT_MODEL, providers=GPU_PROVIDERS).embed([text]))[0], dtype=np.float32)
                q["colbert"] = mv.tolist()
            queries[domain].append(q)
    (DATA / "queries.json").write_text(json.dumps(queries))


def main() -> None:
    DATA.mkdir(parents=True, exist_ok=True)
    report = {}
    for domain, colbert in (("berlin", False), ("legal", True)):
        docs = load_corpus(domain)
        report[domain] = embed_collection(
            domain,
            texts_for_dense=[d["description"] if domain == "berlin" else d["summary"] for d in docs],
            texts_for_sparse=[d["description"] if domain == "berlin" else d["summary"] for d in docs],
            colbert_texts=[d["summary"] for d in docs] if colbert else None,
        )
    embed_queries()

    artifacts = sorted(p.name for p in DATA.iterdir()
                       if p.suffix in (".f32", ".json", ".jsonl"))
    manifest = {
        "generated": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "python": platform.python_version(),
        "onnxruntime": {"version": ort.__version__, "device": ort.get_device(),
                        "providers": ort.get_available_providers()},
        "models": {"dense": DENSE_MODEL, "colbert": COLBERT_MODEL, "sparse": SPARSE_MODEL},
        "queries": QUERIES,
        "report": report,
        "artifacts": {a: {"sha256": sha256(DATA / a), "bytes": (DATA / a).stat().st_size}
                      for a in artifacts},
    }
    (DATA / "manifest.json").write_text(json.dumps(manifest, indent=2))
    print(f"manifest.json written ({len(artifacts)} artifacts)")


if __name__ == "__main__":
    main()
