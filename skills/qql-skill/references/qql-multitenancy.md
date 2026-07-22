# QQL Multi-Tenancy Guide

This reference walks through the complete multi-tenant pattern: one Qdrant collection serving many tenants with full isolation at both the payload and shard level.

## The Problem

You're building a SaaS platform. Users submit natural-language queries. Your platform must:

1. Route each query to the correct tenant's data
2. Prevent cross-tenant data leaks (hard guarantee, not a hope)
3. Work across Python, Rust, Node, and browser WASM
4. Be auditable — every query must leave a trace that's reviewable

## The Solution

Three layers of isolation, expressed in QQL:

```
┌─────────────────────────────────────────────┐
│  Layer 1: Shard routing (physical)          │
│  SHARD 'honeywell' → only touches that shard │
├─────────────────────────────────────────────┤
│  Layer 2: Payload filtering (logical)       │
│  WHERE tenant_id = 'honeywell'              │
├─────────────────────────────────────────────┤
│  Layer 3: AST injection (programmatic)      │
│  inject_filter() ensures filter is ALWAYS   │
│  present before any query reaches Qdrant    │
└─────────────────────────────────────────────┘
```

## Step 1: Define the Collection

```sql
CREATE COLLECTION sec10k HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (
  replication_factor = 2,
  shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx']
);
```

| Parameter | Value | Why |
|-----------|-------|-----|
| `shard_number` | 8 | 4 tenants × 2 shards each |
| `sharding_method` | `'custom'` | Explicit shard-to-tenant mapping |
| `shard_keys` | `['honeywell', ...]` | One shard key per tenant |
| `HYBRID` | dense + sparse | Full hybrid search per tenant |

## Step 2: Optimize Tenant Filtering

```sql
CREATE INDEX ON COLLECTION sec10k FOR tenant_id
  TYPE keyword WITH (is_tenant = true);
```

`is_tenant = true` is a Qdrant-native optimization. It tells Qdrant that `tenant_id`
is the primary partition key, enabling faster filtering for tenant-scoped queries.

## Step 3: Ingestion with Shard Routing

```sql
UPSERT INTO sec10k VALUES
  {id: 1, text: '...risk disclosure...', tenant_id: 'honeywell', fiscal_year: 2024},
  {id: 2, text: '...supply chain...', tenant_id: 'honeywell', fiscal_year: 2024}
  SHARD 'honeywell';

UPSERT INTO sec10k VALUES
  {id: 3, text: '...aviation segment...', tenant_id: 'ge', fiscal_year: 2024}
  SHARD 'ge';
```

`SHARD '<key>'` routes each batch to the correct physical shard. The `tenant_id`
payload field provides the logical filter.

## Step 4: Safe Query Execution

### Python

```python
from pyqql import parse, inject_filter, Client

# User submits a query string
stmt = parse("QUERY 'supply chain risks' FROM sec10k LIMIT 10")

# Platform injects tenant isolation — single call site, covers all paths
inject_filter(stmt, "tenant_id", "=", "honeywell")

# Execute safely
client = Client("http://localhost:6333")
result = client.execute_stmt(stmt)
```

### Rust

```rust
use qql_core::parser::Parser;
use qql_core::ast::{self, ComparisonOp, Value};
use qql::executor::Executor;
use qql::rest::RestQdrant;

async fn execute_for_tenant(exec: &Executor, query: &str, tenant: &str) {
    let mut stmt = Parser::parse(query).unwrap();

    // Inject tenant isolation
    ast::inject_filter(
        &mut stmt, "tenant_id", ComparisonOp::Eq,
        Value::Str(tenant.to_string()),
    ).unwrap();

    // Statement now has WHERE tenant_id = '<tenant>' injected
    let res = exec.execute_node(stmt).await.unwrap();
    println!("{}", res.message);
}
```

### Node.js

```js
import { parse, injectFilter, Client } from 'nqql';

const stmt = parse("QUERY 'supply chain risks' FROM sec10k LIMIT 10");
injectFilter(stmt, "tenant_id", "=", "honeywell");

const client = new Client("http://localhost:6333", null);
const result = client.executeStmt(stmt);
```

## Step 5: Add Shard Routing to the Query

For maximum performance, combine payload filtering with shard routing:

```sql
QUERY 'supply chain risks'
  FROM sec10k
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell'
  LIMIT 10;

-- Count tenant's documents
COUNT FROM sec10k
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell';
```

When you use `inject_filter()` to inject the `WHERE tenant_id = ...` clause, you can
also add `SHARD '<key>'` by mutating the `shard_key` field on the AST statement:

```rust
if let Stmt::Query(ref mut q) = stmt {
    q.shard_key = Some(tenant.to_string());
}
```

`inject_filter()` also works with `COUNT` statements — injected filters are merged into the `WHERE` clause automatically.

## What Makes This Different

### Without QQL
```python
# Must know LlamaIndex internals
from qdrant_client import models
from llama_index.vector_stores.qdrant import QdrantVectorStore
from llama_index.vector_stores.types import MetadataFilter, MetadataFilters

# Every code path that builds a query needs this filter logic
tenant_filter = MetadataFilters(
    filters=[MetadataFilter(key="tenant_id", value="honeywell")]
)

# Must pass shard_key_selector_fn at collection setup time
# Must pass shard_identifier on every async_add call
# Must remember to add the filter to every single query
# One missed code path → data leak
```

### With QQL
```python
# One line. Every time. Recursive. Guaranteed.
inject_filter(stmt, "tenant_id", "=", "honeywell")
```

The difference isn't lines of code. It's that **you cannot forget**. `inject_filter`
recursively descends into every CTE, prefetch, and sub-query. There is no code path
where the filter can be accidentally omitted.

## Verification

```sql
-- Show the execution plan to verify tenant isolation
EXPLAIN QUERY 'risk' FROM sec10k
  WHERE tenant_id = 'honeywell' SHARD 'honeywell' LIMIT 10;
```

Output:
```
Statement: QUERY
Intent: nearest neighbors from text
Collection: sec10k
Filter: present         ← tenant_id = 'honeywell' is in the plan
Limit: 10
```

Every plan is auditable. Every plan is reviewable. Every plan proves the filter is there.
