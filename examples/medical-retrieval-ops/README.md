# Medical Retrieval Operations

This example turns the full [ChatMED-Project/RAGCare-QA](https://huggingface.co/datasets/ChatMED-Project/RAGCare-QA) benchmark into a Qdrant collection with `qql-go`, then runs retrieval and benchmark checks against it.

The indexed text contains the medical context and supporting answer. The benchmark queries are the medical questions. That means the retrieval test is not "store the question and search the same question back."

## Run it

```bash
bash examples/medical-retrieval-ops/run-demo.sh
```

## Benchmark it

```bash
bash examples/medical-retrieval-ops/run-benchmark.sh
```

## Requirements

- `qql-go` on `PATH`
- `uv`
- a working `qql-go` connection that supports text insert and text search

## Dataset

- default dataset: `ChatMED-Project/RAGCare-QA`
- default size: full dataset
- default bulk insert chunk size: `200`

Optional overrides:

```bash
export MEDICAL_RAG_DATASET="ChatMED-Project/RAGCare-QA"
export MEDICAL_RAG_MAX_ROWS="all"
export MEDICAL_RAG_CHUNK_SIZE="200"
```

## Files

- `01-provision.qql`
  Creates the collection and payload indexes.
- `build-medical-corpus.py`
  Downloads the dataset, writes `generated/02-seed.qql`, `generated/eval.json`, and `generated/benchmark-questions.json`.
- `run-demo.sh`
  Loads the full dataset, runs the showcase queries, and writes a multi-mode benchmark artifact.
- `run-benchmark.sh`
  Runs the full generated benchmark and reports `hit@1` and `hit@5` for dense, sparse, hybrid RRF, hybrid DBSF, and exact.
- `agent-playbook.md`
  Minimal agent workflow for answering a medical question from this collection.

## Workflow

1. Download the full benchmark dataset
2. Build a QQL seed file from the context and answer fields
3. Create `medical_retrieval_ops`
4. Insert the generated corpus
5. Compare dense, sparse, hybrid RRF, hybrid DBSF, exact retrieval, score-thresholded retrieval, and offset windows on a benchmark question
6. Filter by tenant, active status, and high priority
7. Group by medical specialty, apply grouped score thresholds, and diversify dense results with MMR
8. Select, recommend, scroll, and dump
9. Run the generated benchmark pack across all retrieval modes
