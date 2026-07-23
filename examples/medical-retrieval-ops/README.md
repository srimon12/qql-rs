# Medical Retrieval Operations

This example turns the full [ChatMED-Project/RAGCare-QA](https://huggingface.co/datasets/ChatMED-Project/RAGCare-QA) benchmark into a Qdrant collection with `qql`, then runs retrieval and benchmark checks against it.

## Run it

```bash
QQL_BIN=./target/release/qql bash examples/medical-retrieval-ops/run-demo.sh
```

## Requirements

- `qql` CLI built from this repo (`cargo build --release -p qql-cli --no-default-features --features rest`)
- `uv`
- a running Qdrant instance at `http://localhost:6333`
