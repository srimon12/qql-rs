# QQL CLI reference

Full `qql` command surface. Global `--url` overrides `QDRANT_URL`. Default is `http://localhost:6333`. URLs with `:6334` select gRPC transport where the command supports it.

## Execute and inspect

```bash
qql exec "QUERY 'hello' FROM docs USING dense LIMIT 5" --json
qql execute script.qql --stop-on-error
qql explain "QUERY 'hello' FROM docs USING dense LIMIT 5" --json
qql fmt script.qql --check
qql fmt script.qql --write
qql connect
qql repl
qql doctor --json
qql version
```

Key decisions:

- `exec` runs one statement. `execute` runs a script file. `--stop-on-error` halts on first failure. `--json` emits machine-readable reports.
- `explain` prints the hierarchical ASCII plan tree with no Qdrant I/O.
- `fmt` canonicalizes QQL. `--check` verifies formatting. `--write` rewrites in place.
- `connect` and `repl` open the interactive shell with multiline input, `\f` format, `\d` doctor, `\e` script helpers.
- `doctor` checks connection health plus loaded model hosts as dense, multi, image, cross_rerank.

## Convert and record

```bash
qql convert search.json
echo '{"ids": [1, "point-2"]}' | qql convert --collection docs
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --out capture.jsonl --qql-out capture.qql
```

Full workflow lives in `convert-migration.md`. `record` needs a `--features record` build. Conversion failures append `# ERROR` lines and never interrupt proxy forwarding.

## Dump and migrate

```bash
qql dump my_collection output.qql
qql dump my_collection output.qql --where "status = 'active'" --batch-size 128
qql migrate docs --to docs_copy --dry-run
qql migrate docs --to docs_copy --recreate --workers 4 --batch-size 128
qql migrate docs --to docs_copy --resume
qql --url grpc://old-host:6334 migrate docs --target-url grpc://new-host:6334 --to docs --target-api-key "$QDRANT_TARGET_API_KEY"
qql --url grpc://localhost:6334 migrate docs --to docs_v2 --recreate --cutover docs
```

Key decisions:

- `dump` writes a collection to a QQL script for backup and restore. Filter with `--where`.
- `migrate` copies schema plus points between clusters or collections. `--dry-run` prints the plan and writes nothing. `--resume` continues from `.qql-migrate/` checkpoint. `--restart` discards it.
- `--to` renames the target. `--target-url` and `--target-api-key` point at a remote target. `--workers` and `--batch-size` tune throughput. `--quantize` rewrites vector spec in flight. `--where` migrates a filtered subset. `--cutover` swaps an alias after verify. `--drop-source-after-cutover` requires `--cutover`.
- Full flag table and six-phase machine live in `convert-migration.md`.

## Edge commands

```bash
qql --edge exec "QUERY 'hello' FROM docs USING dense LIMIT 5"
qql config edge --bm25-k1 2.0 --bm25-b 0.5 --bm25-avg-len 8.0
qql edge bootstrap docs --from http://localhost:6333
qql edge bootstrap docs --from http://localhost:6333 --shard-id 0 --force --json
qql --edge migrate local_docs --target-url http://server:6333 --to docs
qql edge optimize docs --json
qql --edge doctor
qql check --edge
```

Key decisions:

- `--edge` runs against local qdrant-edge with no server. `config edge` persists BM25 and WAL settings.
- `edge bootstrap` seeds a local collection from a remote shard snapshot. Single-shard sources auto-discover. Multi-shard sources need `--shard-id`. Existing locals need `--force`.
- `--edge migrate --target-url` publishes edge data to a server. Edge to edge copies fail closed. Seed other devices with `edge bootstrap`.
- `edge optimize` triggers background optimization. `doctor` and `check --edge` report `indexed_vectors_count` lag.

## Environment

| Variable | Role |
|----------|------|
| `QDRANT_URL` | REST base, default `http://localhost:6333` |
| `QDRANT_API_KEY` | Qdrant API key |
| `QDRANT_TARGET_API_KEY` | Target cluster key for migrate |
| `EMBED_URL` | OpenAI-compatible embeddings endpoint |
| `EMBED_MODEL` | Embedding model name |
| `EMBED_DIM` | Embedding dimension |
| `QQL_EDGE_WAL_SEGMENT_MB` | Edge WAL segment capacity |
| `QQL_EDGE_BM25_K1`, `QQL_EDGE_BM25_B`, `QQL_EDGE_BM25_AVG_LEN` | Edge BM25 document tuning |

## Build features

```bash
cargo build --release -p qql-cli --no-default-features --features rest
cargo build --release -p qql-cli --no-default-features --features rest,grpc
cargo build --release -p qql-cli --features edge
cargo build --release -p qql-cli --features record
```

Binary is `target/release/qql`. Features are `rest`, `grpc`, `edge`, `record`. REST-only builds fastest. gRPC adds tonic transport. Edge adds in-process execution plus fastembed. Record adds the capture proxy.
