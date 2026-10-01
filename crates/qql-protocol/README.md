# qql-protocol

Qdrant REST protocol surface: the closed result IR plus the strict parsers
that turn backend envelopes into it.

## What lives here

- `response` — the closed `ExecData` family (`Hits`, `Groups`, `Count`,
  `Facet`, `Mutation`, `Collections`, `Collection`, `ShardKeys`, `Quotas`),
  scores, and the `ExecResponse` / `ExecutionReport` envelopes.
- `rest` — strict per-operation OpenAPI response parsing. A missing or
  mistyped field fails with `QQL-BACKEND-ENVELOPE`; there is no fallback and
  no raw-JSON passthrough.
- `schema` — collection metadata and the REST schema reader used by `USING`
  resolution.
- `telemetry` — server `time` / `usage` extraction and phase timings.
- `normalize` — pure typed-response → execution-response shaping shared by
  every host.

## Boundaries

No I/O, no networking, no transport clients: this crate only understands the
JSON shapes Qdrant REST emits. `qql-runtime` re-exports the surface so
`qql::executor::*` / `qql::backend::*` stay stable; `qql-wasm` depends on it
directly so browser and native hosts parse identical shapes.
