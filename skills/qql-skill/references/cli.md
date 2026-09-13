# QQL CLI reference

Full `qql` command surface. Global `--url` and `--api-key` override environment variables and `~/.qql/config.json`. Default is `http://localhost:6333`. URLs with `:6334` select gRPC transport where the command supports it.

## Configuration precedence

1. CLI flags: `--url <URL>`, `--api-key <KEY>`
2. Environment variables: `QDRANT_URL`, `QDRANT_API_KEY`
3. Configuration file: `~/.qql/config.json` (managed via `qql setup` or `qql config set`)
4. Default fallback: `http://localhost:6333`

---

## Three-tier mental model

### Tier 1: Offline & CI (No backend connection needed)

```bash
qql lint [file|dir] [--fix] [--write] [--json]
qql fmt script.qql --check
qql fmt script.qql --write
qql explain "QUERY 'hello' FROM docs USING dense LIMIT 5" --json
qql convert search.json
echo '{"ids": [1, "point-2"]}' | qql convert --collection docs
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --out capture.jsonl --qql-out capture.qql
```

- `lint`: Static analysis and plan verification for QQL scripts. Recovers past syntax errors, compiles plans, flags redundant `WITH PAYLOAD true`, renders codeframes, and autofixes with `--fix` / `--write`.
- `fmt`: Canonicalizes QQL. `--check` verifies formatting in CI. `--write` rewrites in place.
- `explain`: Prints hierarchical ASCII plan tree with zero Qdrant I/O. Accepts `-p`/`--param` and `--params-file`.
- `convert`: Translates REST JSON into typed QQL.
- `record`: Captures live proxy traffic and outputs JSONL and converted QQL (`--features record`).

### Tier 2: Live query execution & triage (Scoped to one query)

```bash
qql run "QUERY 'hello' FROM docs USING dense LIMIT 5" --json
qql run script.qql -p category=papers
qql exec "SHOW COLLECTIONS"
qql execute script.qql --stop-on-error
qql doctor "QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 1"
qql check "QUERY 'hello' FROM docs USING dense LIMIT 5"
```

- `run`: Smart runner. Automatically detects whether the argument is an inline query string or a `.qql` script file.
- `exec` and `execute`: Aliases for running an inline string or file respectively.
- `doctor "<query>"` (alias `check`): 5-stage live triage loop (format, offline plan, embedder probe, vector topology, backend health). Exits non-zero on first failure. Note: requires live backend reachability.

### Tier 3: Cluster operations & setup

```bash
qql setup
qql setup --url http://localhost:6333 --yes
qql config show
qql config get url
qql config set url http://localhost:6333
qql config path
qql doctor --json
qql repl
qql dump my_collection output.qql --batch-size 128
qql migrate docs --to docs_copy --dry-run
qql migrate docs --to docs_copy --recreate --workers 4 --batch-size 128
qql version
```

- `setup`: Interactive/non-interactive initial configuration wizard writing to `~/.qql/config.json`.
- `config`: Inspect and update persistent configuration keys (`show`, `get`, `set`, `path`, `edge`).
- `doctor`: Cluster-wide reachability, authentication check, embedder probe, and vector dimension consistency check.
- `repl` (alias `connect`): Interactive shell with multiline buffers, `\f` format, `\d` doctor, `\p` parameter bindings.
- `dump` and `migrate`: Export to `.qql` script or migrate collections across clusters.

---

## Edge commands

```bash
qql --edge run "QUERY 'hello' FROM docs USING dense LIMIT 5"
qql config edge --bm25-k1 2.0 --bm25-b 0.5 --bm25-avg-len 8.0
qql edge bootstrap docs --from http://localhost:6333
qql edge bootstrap docs --from http://localhost:6333 --shard-id 0 --force --json
qql --edge migrate local_docs --target-url http://server:6333 --to docs
qql edge optimize docs --json
qql --edge doctor
```

Key decisions:
- `--edge` runs against local qdrant-edge with no server. `config edge` persists settings.
- `edge bootstrap` seeds a local collection from a remote shard snapshot.
- `edge optimize` triggers background optimization.

---

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

---

## Build features

```bash
cargo build --release -p qql-cli --no-default-features --features rest
cargo build --release -p qql-cli --no-default-features --features rest,grpc
cargo build --release -p qql-cli --features edge
cargo build --release -p qql-cli --features record
```

Binary is `target/release/qql`. Features are `rest`, `grpc`, `edge`, `record`.
