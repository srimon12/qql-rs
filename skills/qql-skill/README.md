# QQL Agent Skill

Packaged guidance for coding agents that author QQL and call SDKs.

## Proposition

- **Language:** one SQL-like grammar for Qdrant retrieval, hybrid, multivector, mutations, DDL.
- **Plan IR:** transport-neutral `PlannedOperation` — gRPC and REST are first-class projections.
- **Isolation:** `inject_filter` on the AST (fail-closed).
- **Routing:** `SHARD '…'` in QQL or `stmt.shard_key` — never inside `Filter`; no `inject_shard_key`.
- **Honesty:** [references/qql-gaps.md](references/qql-gaps.md) lists open vs closed capabilities.

## Layout

| Path | Role |
|------|------|
| [SKILL.md](SKILL.md) | Intent map + compact grammar (load first) |
| [references/](references/) | Deep dives: examples, multitenancy, SDKs, install, gaps |
| [scripts/](scripts/) | Runnable demos (`demo_*.py`) |

## Human product docs

See [`docs/`](../../docs/) for syntax, filters, inject_filter, and history.

## Multitenancy one-liner

```sql
-- DDL: define keys
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);

-- DML: isolate + route
QUERY TEXT 'q' FROM docs USING dense
WHERE tenant_id = 'acme' SHARD 'acme' LIMIT 10;
```

Host: always `inject_filter(..., "tenant_id", "=", tenant)` on untrusted QQL.
