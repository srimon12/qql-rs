# Converting REST JSON to QQL (`qql convert` / `qql record`)

Use when the user has an existing Qdrant REST integration and wants QQL
without hand-translating requests.

## Zero-code-change capture (`qql record`)

`qql record` proxies a live application to Qdrant, forwards every request
unchanged, and appends collection and quota requests as wrapped JSONL plus
optionally converted QQL. Requires a `--features record` CLI build.

```bash
cargo build --release -p qql-cli --features record
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 \
  --out capture.jsonl --qql-out capture.qql
```

Defaults: listen `127.0.0.1:6334`, target `http://127.0.0.1:6333`, out
`capture.jsonl`. Query strings (`wait`, `timeout`, `consistency`) are
forwarded and captured on the wrapped `"query"` object. Conversion failures
append `# ERROR` lines and never interrupt forwarding.

## Convert one request (`qql convert`)

Input is either a wrapped request `{method, path, query?, body?}` (collection
derived from the path; `query` recovers `WAIT` / timeout / consistency) or a
bare Qdrant body (`--collection <name>` required).

```bash
qql convert search.json
echo '{"ids": [1, "point-2"]}' | qql convert --collection docs
# QUERY POINTS (1, 'point-2') FROM docs;
```

## Contract facts (do not guess)

- Output is produced by the same formatter as `qql fmt`; every emitted
  statement re-parses and is canonical.
- Coverage follows the statement → endpoint matrix (the 26 statement routes plus the alias helper):
  query/groups, points, scroll, count, facet, upsert, delete, payload and
  vector mutations, collection/index/shard/quota DDL, and `BATCH` query/mutation blocks.
- Filters: `must` → `AND`, `should` → `OR`, `must_not` → `NOT`; `match`,
  `match_text_any` → `MATCH TOKENS`, `match_except` → `MATCH EXCEPT`,
  `min_should` → `MIN SHOULD`, `range`, geo (`GEO_BBOX` / `GEO_RADIUS` / `GEO_POLYGON`), `has_id`,
  `is_empty`, `is_null` are typed predicates.
- Fields QQL cannot represent fail with a typed `ConvertError`
  (`UnsupportedEndpoint`, `UndecodableBody`, `InvalidField`, `InvalidJson`,
  `MissingCollection`) — never a silent clause drop or placeholder text.
- The alias helper `POST /collections/aliases` is not a QQL statement and is
  reported as `UnsupportedEndpoint`.

## Typical workflow

1. `qql record --out capture.jsonl` (app points at the recorder).
2. `qql convert --collection <name> capture.jsonl` → review plain QQL.
3. `qql check "<statement>"` for staged triage.
4. `qql execute capture.qql` (drop `# ERROR` lines first) or
   [cluster migration](https://qql.sh/docs/operations/cluster-migration/).

Full product docs: `website/src/content/docs/docs/operations/convert.mdoc`.
