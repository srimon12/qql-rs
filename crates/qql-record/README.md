# qql-record

Transparent Qdrant REST traffic recorder and live QQL converter.

`qql-record` is a developer proxy that forwards Qdrant REST traffic unchanged (status, headers, auth, query strings) while recording incoming requests to `.jsonl` and producing converted `.qql` files in real-time.

## Installation

```bash
cargo install --locked qql-record
```

## Usage

```bash
# Capture live REST calls and convert to QQL
qql-record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --out capture.jsonl --qql-out capture.qql
```
