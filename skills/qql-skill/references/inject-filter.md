# `inject_filter` (skill reference)

Host-side **logical isolation**. Recursively ANDs a comparison into the AST before plan/execute.

## Do / don’t

| Do | Don’t |
|----|--------|
| Always inject tenant / org / project on untrusted QQL | Trust the client to send `WHERE tenant_id = …` |
| Use equality (`=`) for policy stamps | Expect `inject_filter` to support `!=` / `IN` / full text |
| Pair with `is_tenant` indexes when useful | Confuse this with `SHARD` routing |
| Fail closed on DDL (validation error is correct) | Use inject for `CREATE COLLECTION` / `SHOW` |

## Isolation vs routing

```text
inject_filter(tenant_id = 'acme')  →  Filter
SHARD 'acme' / stmt.shard_key      →  request shard_key / ShardKeySelector
```

No `inject_shard_key`. Author `SHARD '…'` in QQL when the tenant is known; use
`stmt.shard_key` only when auth resolves the key after parse.

## Signatures

**Rust:** `inject_filter(&mut Stmt, field, ComparisonOp, Value) -> Result<()>`  
**Python:** `inject_filter(stmt|str, field, op, value)` or `stmt.inject_filter(...)`  
**Node:** `injectFilter(str, …)` or `stmt.injectFilter(...)`  
**WASM:** `stmt.injectFilter(...)` (and string free function where exported)

## Propagation

- `QUERY`: top-level + every CTE + nested prefetch queries  
- `SCROLL` / `COUNT`: statement filter  
- Mutations: selector / filter merge  
- `UPSERT` + `Eq`: stamp payload keys  

## Full guide

See [docs/inject_filter.md](../../../docs/inject_filter.md) and
[qql-multitenancy.md](qql-multitenancy.md).
