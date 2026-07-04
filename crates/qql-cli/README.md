# qql-cli

Command Line Interface (CLI) and interactive REPL shell tool for QQL.

---

## Installation

Build the CLI binary using cargo:
```bash
cargo build --release -p qql-cli
```

The resulting binary will be saved at `target/release/qql`.

---

## Command Usage

Execute QQL commands directly from your terminal or launch the interactive shell:

### Run directly
```bash
qql -q "SHOW COLLECTIONS"
```

### Launch Interactive REPL Shell
```bash
qql
```
Inside the interactive REPL shell:
```
qql> SHOW COLLECTIONS;
qql> QUERY 'search terms' FROM docs LIMIT 5;
qql> exit
```

---

## Configuration

The CLI reads configuration from `qql.toml` or environment variables for connection parameters:
* `QDRANT_URL`: Qdrant gRPC/HTTP URL (default: `http://localhost:6334`)
* `INFERENCE_MODE`: Model embeddings provider (`local` or `cloud`)
