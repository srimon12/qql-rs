# QQL conformance

An implementation may claim a conformance level only when every applicable
fixture passes unchanged.

## Levels

| Level | Requirement |
|---|---|
| Syntax | Parse all valid fixtures and reject all invalid fixtures |
| Semantic | Syntax plus language/planner validation and exact invalid-case codes |
| Canonical AST | Semantic plus structural equality with all `qql.ast/v1` snapshots |
| Execution | Optional backend-specific profile; outside base QQL 1.x conformance |

The qql-rs runner checks Syntax, Semantic, and Canonical AST:

```bash
cargo run -p qql-conformance -- check language/v1
```

The path can be omitted from the workspace root or supplied with
`QQL_LANGUAGE_DIR`:

```bash
cargo run -p qql-conformance -- check

QQL_LANGUAGE_DIR=/absolute/path/to/qql-rs/language/v1 \
  cargo run -p qql-conformance -- check
```

## Valid fixtures

Every `fixtures/valid/*.qql` file is one script. Scripts may contain comments
and multiple semicolon-separated statements. Statement order is preserved in
canonical output.

## Invalid fixtures

Invalid files contain isolated programs:

```sql
-- @case invalid-vector-kind
-- @error QQL-PARSE-VECTOR-KIND
QUERY 'search' FROM docs USING lexical_v2 AS HYBRID LIMIT 10;
```

- `-- @case <unique-name>` begins one program.
- `-- @error <code>` declares its exact expected code.
- Source continues until the next case marker or EOF.
- Human-readable messages and span formatting are not compared.

## Canonical AST

Each valid fixture maps to `fixtures/expected/<basename>.json`:

```json
{
  "schema": "qql.ast/v1",
  "statements": []
}
```

JSON object key order is ignored, array order is preserved, and floating
values are rounded to six decimal places. Snapshots are generated artifacts;
do not edit them manually.

### Canonical format

Alongside each AST snapshot, `generate` writes
`fixtures/formatted/<name>.txt`: the exact output of `fmt::format` for the
fixture. This is the canonical-text contract for every formatter
implementation. The native `check` verifies it, and the bundled editor WASM
must reproduce it byte-for-byte (`editors/vscode/test/format.test.js`), so a
stale WASM build fails on the first fixture that exercises a changed surface.

```bash
cargo run -p qql-conformance -- generate language/v1
```

Generation refuses to write until every valid program parses and plans and
every invalid case is rejected with its declared code.

## CI

The repository checks both generated parser freshness and language behavior:

```yaml
- name: Check generated grammar
  run: cargo run -p qql-grammar-gen -- check

- name: Check QQL language conformance
  run: cargo run -p qql-conformance -- check language/v1
```

## Adding or changing syntax

1. Update `grammar.pest`.
2. Add focused valid and invalid fixtures.
3. Regenerate the qql-core parser input.
4. Update AST lowering when required.
5. Run conformance.
6. Regenerate and review snapshots only for intended AST changes.

Another implementation may use any parser technology, but it must accept the
same grammar, enforce equivalent semantic rules, emit the `qql.ast/v1`
envelope, and pass these fixtures unchanged.
