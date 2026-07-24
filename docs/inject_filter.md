# QQL AST Filter Injection & Sandboxing Guide

**Filter Injection (`inject_filter`)** is QQL's zero-trust AST transformation engine. It allows platform code, API gateways, and agent runtimes to inject mandatory security, lifecycle, and policy constraints into any parsed QQL statement **before** it is planned, compiled, or executed against Qdrant.

Unlike manual payload filter construction in raw database clients, `inject_filter` operates directly on the QQL Abstract Syntax Tree (AST). It recursively propagates predicates across complex query graphs, Common Table Expressions (CTEs), prefetch/fusion trees, and mutation statements (`DELETE`, `UPDATE PAYLOAD`, `UPSERT`).

---

## Key Differences: QQL `inject_filter` vs. Raw Qdrant Payload Filtering

| Feature | Raw Qdrant Filter JSON | QQL `inject_filter` AST Engine |
| :--- | :--- | :--- |
| **Enforcement Point** | Client-side manual construction before API call | Server/Gateway AST transform before planning |
| **CTE & Subquery Support** | Manual duplication into every `prefetch` JSON block | Automated recursive injection into all CTE branches & prefetches |
| **Agent / LLM Sandboxing** | Requires parsing/rebuilding raw JSON query objects | Single AST call: `inject_filter(&mut stmt, field, op, value)` |
| **Mutation Guardrails** | Manual filter building for `delete` & `update` | Automatically injects into `DELETE`, `UPDATE PAYLOAD`, & `UPSERT` |
| **Bypass Resistance** | High risk of omitted `must` conditions | Resists `OR` logical bypasses, negations, and empty `WHERE` clauses |

---

## Physical Optimization vs. Logical Injection (`is_tenant = true`)

In Qdrant, payload filtering and physical data layout are complementary:

1. **Logical Isolation (`inject_filter`):**  
   QQL guarantees that a condition like `group_id = 'grp-101'` or `workspace_id = 'ws-5'` is injected into every AST branch. The query author or LLM cannot bypass it.
2. **Physical Layout Optimization (`is_tenant = true`):**  
   You can point `is_tenant = true` to **any keyword field name** (`group_id`, `org_id`, `user_id`, `workspace_id`, etc.) during index creation:
   ```sql
   CREATE INDEX ON docs (group_id KEYWORD WITH (is_tenant = true));
   ```
   Qdrant uses this index to physically co-locate points belonging to the same group/tenant on disk for ultra-fast filtered HNSW graph traversal.

---

## 12 Real-World Use Cases for AST Filter Injection

### 1. Soft-Delete & Lifecycle Preservation
Never return or mutate tombstoned or soft-deleted records.
```rust
inject_filter(&mut stmt, "deleted", ComparisonOp::Eq, Value::Bool(false))?;
```
* **Use case:** E-commerce, CMS, ticket systems where deletes are soft flags. Prevents accidental retrieval of deleted points.

### 2. Environment / Stage Isolation (Shared Cluster)
Run multi-stage data (production, staging, preview) inside a single collection.
```rust
inject_filter(&mut stmt, "env", ComparisonOp::Eq, Value::Str("prod".into()))?;
```
* **Use case:** Staging/Prod demo clusters and preview deployments without creating separate physical collections.

### 3. Data Residency & Regional Governance
Enforce geographic constraints based on user session or legal requirements.
```rust
inject_filter(&mut stmt, "region", ComparisonOp::Eq, Value::Str("eu-west-1".into()))?;
```
* **Use case:** GDPR and regional compliance for SaaS applications.

### 4. Visibility & Authorization ACLs
Restrict searches to public or user-permitted visibility tiers.
```rust
inject_filter(&mut stmt, "visibility", ComparisonOp::Eq, Value::Str("public".into()))?;
```
* **Use case:** Knowledge bases, Notion-style workspaces, and partner APIs.

### 5. Feature Flags & Corpus Rollouts
Scope searches to specific index versions or experiment buckets.
```rust
inject_filter(&mut stmt, "corpus_version", ComparisonOp::Eq, Value::Str("v3".into()))?;
```
* **Use case:** A/B testing retrieval pipelines and zero-downtime model migration.

### 6. Language & Locale Scoping
Enforce request-level language boundaries on LLM-generated queries.
```rust
inject_filter(&mut stmt, "lang", ComparisonOp::Eq, Value::Str("en".into()))?;
```
* **Use case:** Support bots and international product catalogs.

### 7. Safety Rails & Content Moderation
Ensure untrusted or model-generated queries never expose unapproved content.
```rust
inject_filter(&mut stmt, "moderation_status", ComparisonOp::Eq, Value::Str("approved".into()))?;
```
* **Use case:** User-generated content (UGC) search and media marketplaces.

### 8. Effective Date / Business Rules
Restrict results to currently active price books or legal clauses.
```rust
inject_filter(&mut stmt, "is_current", ComparisonOp::Eq, Value::Bool(true))?;
```
* **Use case:** Insurance policy search, price catalogs, and legal databases.

### 9. Product Surface & Channel Scoping
Vary available points by client surface (mobile app vs. partner API).
```rust
inject_filter(&mut stmt, "channel", ComparisonOp::Eq, Value::Str("mobile_app".into()))?;
```
* **Use case:** Omni-channel inventory and API tiers.

### 10. Agent & LLM Sandbox Guardrails
Safely execute free-form QQL generated by LLMs or external agents.
```rust
// Scope agent to the current session user and active project
inject_filter(&mut stmt, "owner_id", ComparisonOp::Eq, Value::Str(user_id))?;
inject_filter(&mut stmt, "project_id", ComparisonOp::Eq, Value::Str(project_id))?;
```
* **Use case:** RAG gateways, IDE copilots, and AI agents with database access.

### 11. Scoped Mutations (`DELETE`, `UPDATE PAYLOAD`, `CLEAR PAYLOAD`)
Prevent accidental broad deletions or payload wipes by forcing scope filters onto mutation statements.
```rust
inject_filter(&mut delete_stmt, "project_id", ComparisonOp::Eq, Value::Str(project_id))?;
```
* **Use case:** Admin UIs, agent tools, and multi-tenant cleanup jobs.

### 12. Data Provenance & Ingestion Stamping on `UPSERT`
When injecting equality filters on `UPSERT` statements, QQL automatically stamps payload fields on inserted/updated points.
```rust
inject_filter(&mut upsert_stmt, "ingested_by", ComparisonOp::Eq, Value::Str("sync-pipeline-v2".into()))?;
```
* **Use case:** Data pipeline auditing and ingestion tracking.

---

## Supported QQL Filter Conditions & Qdrant Mapping

QQL supports the full suite of Qdrant payload conditions, fully compatible with AST injection:

| QQL Condition | Description | Qdrant Engine Mapping |
| :--- | :--- | :--- |
| `field = val` | Exact match | `MatchValue` |
| `field IN (a, b)` | Match any | `MatchAny` |
| `field NOT IN (a, b)` | Match except | `MatchExcept` |
| `field MATCH TEXT 'phrase'` | Substring / Token search | `MatchText` / `MatchTextAny` |
| `field MATCH PHRASE 'exact'` | Exact phrase match | `MatchPhrase` |
| `field BETWEEN a AND b` | Range comparison | `Range` (`gte`, `lte`) |
| `field IS NULL` | Null value check | `IsNull` |
| `field IS EMPTY` | Empty array/null check | `IsEmpty` |
| `HAS VECTOR 'name'` | Named vector check | `HasVector` |
| `id = 'uuid'` | Point ID predicate | `HasId` |
| `GEO_BBOX(...)` | Geo bounding box | `GeoBoundingBox` |
| `GEO_RADIUS(...)` | Geo radius circle | `GeoRadius` |
| `GEO_POLYGON(...)` | Geo polygon area | `GeoPolygon` |

---

## Multi-Language Usage Examples

### Rust (`qql-core`)
```rust
use qql_core::ast::{inject_filter, ComparisonOp, Value};
use qql_core::parser::Parser;

let mut stmt = Parser::parse("QUERY 'laptops' FROM products LIMIT 10")?;
inject_filter(
    &mut stmt,
    "group_id",
    ComparisonOp::Eq,
    Value::Str("org_99".into()),
)?;
```

### Python (`pyqql`)
```python
import pyqql

stmt = pyqql.parse("QUERY 'laptops' FROM products LIMIT 10")
pyqql.inject_filter(stmt, "group_id", "=", "org_99")
```

### Node.js (`nqql`)
```javascript
const nqql = require('nqql');

const stmt = nqql.parse("QUERY 'laptops' FROM products LIMIT 10");
nqql.injectFilter(stmt, "group_id", "=", "org_99");
```

### WebAssembly (`qql-wasm`)
```javascript
import init, { parse, inject_filter } from 'qql-wasm';

await init();
let stmt = parse("QUERY 'laptops' FROM products LIMIT 10");
inject_filter(stmt, "group_id", "=", "org_99");
```
