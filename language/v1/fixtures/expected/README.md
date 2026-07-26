# Canonical AST snapshots

Each JSON file is generated from the valid QQL fixture with the same basename.
Together these files are the normative `qql.ast/v1` schema.

## Envelope

```json
{
  "schema": "qql.ast/v1",
  "statements": [
    {
      "Query": {
        "collection": {"Explicit": "docs"},
        "ctes": [],
        "expression": {
          "Nearest": {
            "input": {"Text": {"text": "keyword search", "model": null}},
            "using": {"name": "lexical_v2", "kind": "Sparse"},
            "prefetch": [],
            "mmr": null
          }
        },
        "filter": null,
        "params": null,
        "score_threshold": null,
        "group": null,
        "output": {"payload": null, "vectors": null},
        "page": {"limit": 10, "offset": null},
        "shard_key": null
      }
    }
  ]
}
```

Statement and expression enums use externally tagged JSON. Unit variants, such
as `ShowCollections`, are strings. Optional fields are present as `null`.
Ordered payload/config entries remain arrays of `[key, value]` pairs.

Vector targets are value objects:

```json
{"name": "semantic_v2", "kind": "Dense"}
{"name": "lexical_v2", "kind": "Sparse"}
{"name": "legacy_untyped_name", "kind": null}
```

An omitted `USING` target is `null`. Schema inference happens during execution
preparation and does not rewrite canonical parse snapshots.

## Number normalization

Integers remain JSON integers. Floating values are rounded to six decimal
places by the conformance adapter, removing binary32 serialization noise.
JSON object key order is ignored; array order is significant.

## Generation and verification

From the qql-rs workspace root:

```bash
cargo run -p qql-conformance -- generate language/v1
cargo run -p qql-conformance -- check language/v1
```

`generate` first requires all valid programs to parse and pass planner
validation and all invalid cases to fail with their declared codes. It then
writes all snapshots atomically.

Do not edit generated JSON manually. Change the grammar/semantics and source
fixture, update the reference implementation when required, then regenerate.
