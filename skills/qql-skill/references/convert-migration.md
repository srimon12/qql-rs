# Convert and migration reference

Zero-code capture, REST JSON conversion, cluster migration, edge seed and publish, dump and restore. One file because the workflow is one pipeline. Capture, convert, check, move or replay.

## Zero-code capture with qql record

Problem: an existing Qdrant app speaks REST JSON. Rewriting by hand is slow and lossy.

```bash
cargo build --release -p qql-cli --features record
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --out capture.jsonl --qql-out capture.qql
qql record
```

Key decisions:

- `record` is a transparent proxy. Point the app at `--listen` instead of Qdrant. Every request forwards unchanged. Each bodied `/collections/` request appends to `--out` as wrapped JSONL plus optionally converted QQL to `--qql-out`.
- Defaults work locally. Qdrant stays on `127.0.0.1:6333`. Recorder listens on `127.0.0.1:6334`. Bare `qql record` needs only the app base URL changed.
- Query strings forward and capture on the wrapped `query` object, so `WAIT`, `timeout`, and `consistency` survive conversion.
- Bodies buffer in memory per request. Fine for interactive captures and large batched upserts. This is a development tool, not a production proxy.
- Bodyless collection and quota routes (`SHOW`, `DROP COLLECTION`, `DROP INDEX`) record too.
- Conversion failures append `# ERROR` lines and never interrupt forwarding.

## Convert one request with qql convert

```bash
qql convert search.json
cat search.json | qql convert
echo '{"ids": [1, "point-2"]}' | qql convert --collection docs
qql convert --collection docs capture.jsonl
```

Wrapped request input carries the endpoint, so the collection derives from the path:

```json
{
  "method": "POST",
  "path": "/collections/docs/points/query",
  "body": {
    "query": { "nearest": [0.1, 0.2] },
    "using": "dense",
    "limit": 5,
    "filter": { "must": [{ "key": "status", "match": { "value": "active" } }] }
  }
}
```

Converts to:

```sql
QUERY [0.1, 0.2]
FROM docs
USING dense
WHERE status = 'active'
LIMIT 5;
```

Bare bodies have no path. Pass `--collection` explicitly. Without it, bare input fails with `MissingCollection` rather than inventing a name.

```bash
echo '{"ids": [1, "point-2"]}' | qql convert --collection docs
```

```sql
QUERY POINTS (1, 'point-2') FROM docs;
```

## Paste shapes: HTTP snippets and curl

Docs and blogs print `METHOD /path` plus a JSON body, or full `curl` commands — neither is the wrapped envelope. `convert` accepts both pastes directly (offline, no connection). Snippets take full URLs and an `HTTP/x` suffix; curl takes `-X`, `-d`/`--data*`/`--json`, `--header`, `\`-continuations, and several commands. A missing `-X` infers the method (`--data` implies POST, else GET). Fences strip as paste noise.

```bash
# Snippet exactly as docs print it
qql convert <<'EOF'
POST /collections/docs/points/query
{"query": {"nearest": [0.1, 0.2]}, "using": "dense", "limit": 5}
EOF

# curl exactly as terminal history has it
curl -X POST http://localhost:6333/collections/docs/points/count \
  -H 'Content-Type: application/json' \
  -d '{"exact": true}' | qql convert
```

Fail-closed paste rules: `$VAR` / `$(…)` / backticks, `--data @file`, pipes, `--data-urlencode` / `--config` / `-T`, and placeholder collections (`<your-collection>`, `{collection_name}`) all error with a typed `ConvertError` instead of emitting QQL that cannot re-parse. Replace placeholders and variables with literal values before converting.

## Coverage and contract facts

Output uses the same formatter as `qql fmt`. Every emitted statement re-parses and is canonical.

| Request | QQL |
|---|---|
| `POST .../points/query`, `.../query/groups` | `QUERY`, including formula, fusion, prefetch, grouped forms |
| `POST .../points/scroll`, `.../points/count`, `.../facet` | `SCROLL`, `COUNT`, `FACET` |
| `POST .../points`, `.../points/delete` | `QUERY POINTS`, `DELETE` |
| `POST .../points/payload`, `payload/clear`, `payload/delete` | `UPDATE ... SET PAYLOAD`, `CLEAR PAYLOAD`, `DELETE PAYLOAD` |
| `PUT .../points/vectors`, `POST .../points/vectors/delete` | `UPDATE ... SET VECTOR`, `DELETE VECTOR` |
| `PUT .../points` | `UPSERT` |
| `PUT`, `PATCH`, `DELETE /collections/{name}` | `CREATE COLLECTION`, `ALTER COLLECTION`, `DROP COLLECTION` |
| `PUT .../index`, `DELETE .../index/{field}` | `CREATE INDEX`, `DROP INDEX` |
| Shard keys, quotas, `SHOW` metadata | `CREATE` and `DROP SHARD KEY`, `SET QUOTA`, `SHOW ...` |

Filter conversion is structural. `must` becomes `AND`. `should` becomes `OR`. `must_not` becomes `NOT`. `match` and `range` and geo shapes and `has_id` and `is_empty` and `is_null` lower onto typed predicates. `match_text_any` becomes `MATCH TOKENS`. `match_except` becomes `MATCH EXCEPT`. `min_should` becomes `MIN SHOULD`.

Fields QQL cannot represent fail with typed `ConvertError` values `UnsupportedEndpoint`, `UndecodableBody`, `InvalidField`, `InvalidJson`, `MissingCollection`. Never a silent clause drop. The alias helper `POST /collections/aliases` is not a QQL statement and reports `UnsupportedEndpoint`.

## Typical convert workflow

1. `qql record --out capture.jsonl` with the app pointed at the recorder.
2. `qql convert --collection <name> capture.jsonl` and review plain QQL.
3. `qql check "<statement>"` for staged triage, or `qql explain` for plan inspection.
4. `qql execute capture.qql` after dropping `# ERROR` lines, or migrate data when the target is a new cluster.

Converted output is normal QQL. No compatibility mode. It runs with `qql exec` and `qql execute` and SDK `Client.execute`.

## Cluster migration with qql migrate

Problem: copy schema plus points to another cluster or collection with reshard, re-quantize, filtered extract, verify, and alias cutover.

```bash
qql --url grpc://localhost:6334 migrate docs --to docs_copy --recreate
qql --url grpc://old-host:6334 migrate docs --target-url grpc://new-host:6334 --to docs --target-api-key "$QDRANT_TARGET_API_KEY"
qql migrate docs --to docs_copy --dry-run
qql migrate docs --to docs_copy --recreate --workers 4 --batch-size 128
qql migrate docs --to docs_copy --resume
qql --url grpc://localhost:6334 migrate docs --to docs_v2 --recreate --cutover docs
qql --url grpc://old:6334 migrate docs --target-url grpc://new:6334 --to docs_v2 --cutover docs --drop-source-after-cutover
```

Key decisions:

- Migrate is a logical streaming copy. It scrolls source points and upserts into a freshly created target, which rebuilds HNSW from ingested points. It is not a snapshot. No segment files move. Target pays fresh-build indexing cost at about twice working memory during ingest. In return the copy crosses minor versions, changes shard counts, switches quantization, and selects point subsets. For identical version and topology with fastest restore, use a Qdrant native snapshot instead.
- Source is `--url` or `QDRANT_URL`, default `http://localhost:6333`. Target defaults to source unless `--target-url` points elsewhere, with `--target-api-key` or `QDRANT_TARGET_API_KEY` for protected targets. `--to` renames. Without it the target keeps the source name. URLs with `:6334` select gRPC, the recommended ingest path.
- Migrating onto the same endpoint with the same name is rejected. Pass `--to`, `--recreate`, or a different `--target-url`.
- `--dry-run` prints `CREATE COLLECTION`, `CREATE INDEX`, `CREATE SHARD KEY`, and `ALTER COLLECTION` plan with source counts and writes nothing.
- `--resume` continues from an existing checkpoint and errors when none exists. `--restart` discards and starts over. They cannot combine.
- Six phases run in order. Validate, schema, ingest, optimize restore, verify, cutover. Checkpoint records the reached phase. Ctrl+C stops ingest, still restores optimizer threshold, and re-running resumes from the cursor.
- Checkpoints live under `.qql-migrate/` with both cluster identities in the name. Each save is atomic and stores phase, cursor, written and skipped and batch counters, exact source count, original indexing threshold, and fast-bulk flag. Resume compatibility checks source, target, URLs, and the options fingerprint. Mismatch fails and tells you to pass `--restart`.
- Shard discovery with `--shard-key-field <field>` probes with `FACET <field> FROM <coll> LIMIT 10000 EXACT true`, narrowed by `--where`. Fallback is payload-only scroll at 512 points per page with vectors off when facet truncates, returns empty, or fails for missing index. Discovered keys are created in schema phase. Missing keys at ingest are created on demand. Points without the field follow `--on-missing-shard-key` as `error`, `skip`, or `default=<key>`. Use `--shard-key <literal>` when every point shares one key. On standalone Qdrant, custom shard keys fail fast in schema phase with a hint to drop shard flags or target clustered Qdrant.
- Shard keys are typed. All-digit literals are numeric. Everything else is keyword. `SHARD 101` reaches the numeric partition. `SHARD 'acme'` reaches the keyword partition. Bound placeholders follow normal binding. Strings become keywords. Non-negative integers become numbers.
- `--quantize scalar|binary|product|turbo` rewrites vector spec on create. Scalar defaults to `always_ram = true`, `quantile = 0.99`. Binary defaults to `always_ram = true`, `encoding = one_bit`. Product defaults to `always_ram = true`, `compression = x16`. Turbo defaults to `always_ram = true`, `bits = 2`. `--no-always-ram` stores quantized vectors on disk. Without `--quantize`, source quantization copies unchanged.
- Fast-bulk is on by default. It raises `indexing_threshold` to `--bulk-threshold-kb` (default `2000000`) during ingest, then restores with `ALTER COLLECTION`. Lower the threshold on small nodes. `--no-fast-bulk` keeps indexing active for small collections.
- Batch sizing follows bulk-upload guidance. Batches of 64 to 256 points with 2 to 4 streams. Defaults are `--batch-size 128`, `--workers 2`. Multivector collections use `--batch-size 64 --workers 2`. Scroll pages accept up to 64 MiB.
- Verify compares filtered source count against unfiltered target count. A filtered copy verifies against the filtered count. With durable `WAIT true`, one exact read per side suffices. With `--no-wait`, verify polls up to 40 times at 50 ms. `--no-verify` skips comparison. Keep verification on for cutovers.
- `--cutover <alias>` points the alias at the target in one `change_aliases` batch after verification, falling back to create when missing. `--drop-source-after-cutover` drops the source and is rejected without `--cutover`. `--json` reports `written`, `source_count`, `target_count`, `verified`, `resumed`, `cutover_alias`, `source_dropped`, and plan blocks.

Full flag table:

| Flag | Purpose |
|---|---|
| `--to <name>` | Target collection name |
| `--target-url <URL>` | Target endpoint |
| `--target-edge` | Local edge backend as target |
| `--source-edge` | Local edge backend as source, also implied by global `--edge` |
| `--target-api-key <key>` | Target cluster key |
| `--batch-size N` | Scroll and upsert batch size, default `128` |
| `--workers N` | Concurrent upsert streams, default `2` |
| `--shard-number N` | Override target `shard_number` |
| `--replication-factor N` | Override target `replication_factor` |
| `--sharding-method auto\|custom` | Override sharding method |
| `--quantize scalar\|binary\|product\|turbo` | Apply quantization on create |
| `--no-always-ram` | Quantized vectors on disk |
| `--quantize-quantile F` | Scalar quantile, default `0.99` |
| `--quantize-compression x4\|x8\|x16\|x32` | Product compression, default `x16` |
| `--quantize-encoding <enc>` | Binary encoding, default `one_bit` |
| `--quantize-bits 1\|1.5\|2\|4` | Turbo width, default `2` |
| `--shard-key <key>` | Route every upsert to one literal key |
| `--shard-key-field <field>` | Route each point by payload value |
| `--on-missing-shard-key error\|skip\|default=<key>` | Missing-field policy, default `error` |
| `--bulk-threshold-kb N` | Indexing threshold during bulk, default `2000000` |
| `--cutover <alias>` | Alias to target after verify |
| `--drop-source-after-cutover` | Drop source after cutover, requires `--cutover` |
| `--where "<filter>"` | Filtered subset with QQL filter |
| `--checkpoint <path>` | Checkpoint file |
| `--resume` | Resume from checkpoint |
| `--restart` | Discard checkpoint and start over |
| `--dry-run` | Print plan, write nothing |
| `--no-fast-bulk` | Keep indexing during ingest |
| `--no-verify` | Skip exact count verify |
| `--no-wait` | Skip `WAIT true` on upserts |
| `--recreate` | Drop target before create |
| `--json` | JSON output for scripting |
| `-q`, `--quiet` | Quiet mode |

## Edge seed and publish

```bash
qql edge bootstrap docs --from http://localhost:6333
qql edge bootstrap docs --from http://localhost:6333 --shard-id 0 --force --json
qql --edge migrate local_docs --target-url http://server:6333 --to docs
```

Key decisions:

- `edge bootstrap` seeds a local edge collection from a remote shard snapshot. It streams `GET /collections/{c}/shards/{id}/snapshot`, unpacks with the engine snapshot API, verifies by loading the shard, then swaps into the edge data directory. Source config, built HNSW, and quantized data are preserved.
- Single-shard sources auto-discover. Multi-shard sources fail closed with `QQL-SNAPSHOT-SHARD` unless `--shard-id` is given. Existing locals replace only after snapshot verifies, or immediately with `--force`.
- Edge to remote publish is supported. `qql --edge migrate <coll> --target-url <url>` or `--source-edge` streams points to the server. Edge to edge copies fail closed before executors start. A local directory is not a network endpoint. Seed other devices with `edge bootstrap`. Continuous bidirectional sync is not provided. The documented pattern is dual-write plus partial snapshots against a server collection.
- Never tar or copy shard directories by hand. Back up via server snapshots or `qql dump`. Edge snapshot creation is unsupported. qdrant-edge 0.8 unpacks and applies only.

## Dump and restore

```bash
qql dump my_collection output.qql
qql execute output.qql --stop-on-error
```

`dump` writes a collection to a QQL script for backup, inspection, and restore. Replay with `qql execute`. For cluster moves with reshard or requantize, prefer `migrate`. For traffic capture, prefer `record` plus `convert`.

Full product docs live at `website/src/content/docs/docs/operations/convert.mdoc` and `cluster-migration.mdoc`.
