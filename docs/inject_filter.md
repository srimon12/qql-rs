# Host Isolation: `inject_filter`

**Purpose:** force a predicate onto untrusted or agent-written QQL **before** plan/execute — so tenants, soft-deletes, and policy flags cannot be omitted.

This is **logical isolation** only. For physical routing on custom-sharded collections, use QQL `SHARD '…'` or `stmt.shard_key` (see [syntax.md](syntax.md#shard-key-ddl-vs-shard-routing) and [qql-multitenancy](../skills/qql-skill/references/qql-multitenancy.md)). There is **no** `inject_shard_key`.

---

## Why it exists

| Approach | Risk |
|----------|------|
| Ask the client to “remember” `WHERE tenant_id = …` | One missing path → data leak |
| Filter results after retrieval | Wasted work; still wrong if LIMIT/group cuts the set |
| Rebuild nested Qdrant JSON by hand | CTEs / prefetches / hybrids easy to miss |
| **`inject_filter` on the AST** | One call; recurses into CTEs and prefetches; fail-closed on unsupported stmts |

---

## What it does

Given a parsed `Stmt`, merge `field op value` into every applicable branch:

| Statement | Effect |
|-----------|--------|
| `QUERY` | AND into top-level filter; recurse CTEs + nested prefetches |
| `SCROLL`, `COUNT` | Merge into statement filter |
| `DELETE`, `UPDATE … PAYLOAD`, `CLEAR PAYLOAD`, `DELETE PAYLOAD`, `DELETE VECTOR` | Wrap / merge into point selector |
| `UPSERT` | Equality on non-`id` fields stamps payload on each point |
| DDL / `SHOW` | **Error** (`QQL-VALIDATION-FILTER-INJECT`) — fail closed |

**Operators (SDK string form):** `=`, `>`, `>=`, `<`, `<=` (and aliases `eq`/`gt`/…).  
**Not supported:** `!=`, `IN`, `MATCH`, … — put those in authored QQL, or inject equality and compose with `NOT` in the source.

---

## Isolation vs routing vs index layout

```
inject_filter(tenant_id = 'acme')     →  Filter (security)
SHARD 'acme'  /  stmt.shard_key       →  ShardKeySelector (routing / perf)
CREATE INDEX … is_tenant = true       →  Qdrant layout optimization
CREATE SHARD KEY 'acme' …             →  define custom partition (DDL)
```

All four can apply on a multi-tenant collection; only the first is mandatory for isolation.

---

## SDKs

### Rust

```rust
use qql_core::ast::{inject_filter, ComparisonOp, Value};
use qql_core::parser::Parser;

let mut stmt = Parser::parse("QUERY TEXT 'laptops' FROM products USING dense LIMIT 10")?;
inject_filter(
    &mut stmt,
    "tenant_id",
    ComparisonOp::Eq,
    Value::Str("org_99".into()),
)?;
// Optional routing (custom sharding only):
// stmt.set_shard_key(Some("org_99".into()));
// Or author: ... SHARD 'org_99' LIMIT 10
```

### Python (`pyqql`)

```python
import pyqql

stmt = pyqql.parse("QUERY TEXT 'laptops' FROM products USING dense LIMIT 10")[0]
pyqql.inject_filter(stmt, "tenant_id", "=", "org_99")
# or: stmt.inject_filter("tenant_id", "=", "org_99")

# Prefer SHARD in QQL when the key is known up front:
# QUERY ... SHARD 'org_99' LIMIT 10
# Host-resolved after parse:
stmt.shard_key = "org_99"
```

### Node (`@veristamp/nqql`)

```js
const { parse, injectFilter } = require("@veristamp/nqql");

const [stmt] = parse("QUERY TEXT 'laptops' FROM products USING dense LIMIT 10");
stmt.injectFilter("tenant_id", "=", "org_99");
// or: injectFilter(qqlString, "tenant_id", "=", "org_99")

stmt.shardKey = "org_99"; // optional routing; same as SHARD 'org_99'
```

### WASM (`qql-wasm`)

```js
import init, { Stmt } from "qql-wasm";
await init();

const stmt = new Stmt("QUERY TEXT 'laptops' FROM products USING dense LIMIT 10");
stmt.injectFilter("tenant_id", "=", "org_99");
stmt.shardKey = "org_99";
```

---

## Typical policies (equality inject)

| Policy | Inject |
|--------|--------|
| Multi-tenant SaaS | `tenant_id = '…'` |
| Soft delete | `deleted = false` |
| Environment | `env = 'prod'` |
| Region / residency | `region = 'eu-west-1'` |
| Content moderation | `moderation_status = 'approved'` |
| Agent sandbox | `project_id = '…'` (and often tenant) |
| UPSERT provenance | stamp `ingested_by = 'pipeline-v2'` |

Pair with:

```sql
CREATE INDEX ON COLLECTION docs FOR tenant_id
  TYPE keyword WITH (is_tenant = true);
```

---

## Verify before execute

```bash
qql explain "QUERY TEXT 'x' FROM docs WHERE tenant_id = 'acme' LIMIT 5"
# Filter: present
```

In code, re-`explain` / `to_dict` / `toObject` after inject and assert the filter is present before `execute`.

---

## Related

- [filters.md](filters.md) — full `WHERE` surface  
- [syntax.md](syntax.md) — `SHARD` / `CREATE SHARD KEY`  
- Skill: [inject-filter.md](../skills/qql-skill/references/inject-filter.md), [qql-multitenancy.md](../skills/qql-skill/references/qql-multitenancy.md)
