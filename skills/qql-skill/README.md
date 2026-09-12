# QQL Agent Skill

Packaged guidance for coding agents that author QQL and call SDKs.

## Proposition

- Language: one SQL-like grammar for Qdrant retrieval, hybrid, multivector, mutations, DDL. Qdrant 1.19 and QQL 1.7 surface with placeholders, `WAIT`, typed shard keys, quotas, memory and `turbo4`, `MATCH PREFIX`, `SLICE`, `PARAMS (idf = ...)`.
- Plan IR: transport-neutral `PlannedOperation`. gRPC and REST are first-class projections. Quotas are REST-only.
- Isolation: `inject_filter` on the AST, fail-closed.
- Routing: `SHARD '...'` in QQL or `stmt.shard_key`. Never inside `Filter`. No `inject_shard_key`.
- Read affinity: client transport option on all remote SDKs. Rust `with_route_affinity`, `pyqql.Client(route_affinity=...)`, `nqql` `{ routeAffinity }`, WASM `setRouteAffinity`. Not QQL syntax.
- Honesty: `references/qql-gaps.md` lists open versus closed capabilities.

## Layout

| Path | Role |
|------|------|
| `SKILL.md` | Router with intent map and clause order. Load first |
| `references/qql-query.md` | All 13 `QUERY` forms, prefetch, fusion, rerank, formula |
| `references/qql-filters.md` | All 20 filter forms |
| `references/qql-mutations.md` | Upserts, deletes, payload and vector writes |
| `references/qql-read.md` | Scroll, count, facet, group by, batch |
| `references/qql-ddl.md` | Collections, indexes, shard keys, quotas, storage |
| `references/qql-params.md` | Placeholders, binding, prepared statements, inference inputs |
| `references/qql-embeddings.md` | Using roles, BM25, CLIP, ColBERT, model hosts |
| `references/qql-multitenancy.md` | Shard DDL versus routing versus isolation |
| `references/inject-filter.md` | Fail-closed policy injection |
| `references/convert-migration.md` | Convert, record, cluster and edge migration, dump |
| `references/cli.md` | Full CLI surface |
| `references/qql-install.md` | Install matrix and backend versions |
| `references/qql-gaps.md` | Open versus closed |
| `references/python-sdk.md` | `pyqql` full guide, self-contained |
| `references/node-sdk.md` | `nqql` full guide, self-contained |
| `references/wasm-sdk.md` | `qql-wasm` full guide, self-contained |
| `references/rust-sdk.md` | `qql-core` and `qql-plan` and `qql` full guide, self-contained |
| `examples/` | Pure QQL files grouped by task |
| `scripts/` | Runnable Python demos |

Each SDK reference runs alone. Shared language rules live in `qql-query.md` and `qql-filters.md` and `qql-params.md`. SDK guides repeat the minimum needed to copy and run without opening three files.

## Human product docs

See `docs/` for syntax, filters, inject_filter, and history. See `website/src/content/docs/` for convert, cluster migration, CLI, and edge operations.

## Multitenancy one-liner

```sql
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);

QUERY TEXT 'q' FROM docs USING dense
WHERE tenant_id = 'acme' SHARD 'acme' LIMIT 10;

QUERY TEXT 'q' FROM docs USING sparse
WHERE tenant_id = 'acme' SHARD 'acme'
PARAMS (idf = WHERE tenant_id = 'acme')
LIMIT 10;
```

Host: always `inject_filter(..., "tenant_id", "=", tenant)` on untrusted QQL. IDF stays in `PARAMS`. No host inject. No JSON corpus object.
