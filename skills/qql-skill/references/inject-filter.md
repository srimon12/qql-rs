# QQL AST Filter Injection Reference (`inject_filter`)

`inject_filter` is a zero-trust AST modification utility in QQL. It programmatically mutates a parsed QQL statement (`Stmt`) by injecting a mandatory filter predicate across all statement types and sub-graphs **before** planning or execution.

---

## When to Use `inject_filter`

Use `inject_filter` whenever platform code or AI agents receive untrusted QQL queries, or when system-wide context policies must be strictly enforced:

* **Security & Multi-Tenancy:** Enforcing `tenant_id = '...'` or `org_id = '...'`
* **Agent Sandboxing:** Locking LLM-generated QQL to an active `project_id` or `user_id`
* **Soft-Deletes:** Forcing `deleted = false` or `status = 'active'`
* **Safety Rails:** Forcing `moderation_status = 'approved'` or `nsfw = false`
* **Environment Boundaries:** Forcing `env = 'prod'` or `region = 'eu-west-1'`
* **Scoped Mutations:** Preventing unscoped `DELETE`, `UPDATE PAYLOAD`, or `CLEAR PAYLOAD` execution

---

## How `inject_filter` Operates on the AST

When `inject_filter(stmt, field, op, value)` is called:

1. **Queries (`Stmt::Query`):**
   * Merges the filter predicate into the top-level `WHERE` clause (`AND` conjunction).
   * Recursively traverses all Common Table Expression (CTE) definitions in `WITH cte_name AS (...)` blocks.
   * Recursively traverses prefetch and fusion trees (`HYBRID`, `RERANK`, `FUSION`).

2. **Scroll & Count (`Stmt::Scroll`, `Stmt::Count`):**
   * Merges the injected predicate into the statement's filter expression.

3. **Mutations (`Stmt::Delete`, `Stmt::UpdatePayload`, `Stmt::ClearPayload`):**
   * Converts point selectors to filter conditions so deletions and payload updates cannot touch unauthorized points.

4. **Upserts (`Stmt::Upsert`):**
   * When injecting an equality filter (`ComparisonOp::Eq`), it stamps the payload field onto every point payload in the batch for data provenance.

---

## Code Signatures

### Rust (`qql-core`)
```rust
use qql_core::ast::{inject_filter, ComparisonOp, Value};
use qql_core::parser::Parser;

let mut stmt = Parser::parse("QUERY 'laptops' FROM products LIMIT 10")?;
inject_filter(&mut stmt, "group_id", ComparisonOp::Eq, Value::Str("grp_123".into()))?;
```

### Python (`pyqql`)
```python
import pyqql

stmt = pyqql.parse("QUERY 'laptops' FROM products LIMIT 10")
pyqql.inject_filter(stmt, "group_id", "=", "grp_123")
```

### Node.js (`nqql`)
```javascript
const nqql = require('nqql');

const stmt = nqql.parse("QUERY 'laptops' FROM products LIMIT 10");
nqql.injectFilter(stmt, "group_id", "=", "grp_123");
```

### WASM (`qql-wasm`)
```javascript
import init, { parse, inject_filter } from 'qql-wasm';

await init();
let stmt = parse("QUERY 'laptops' FROM products LIMIT 10");
inject_filter(stmt, "group_id", "=", "grp_123");
```
