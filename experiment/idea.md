# QQL Agent Memory & Universal CLI Architecture (idea.md)

## Executive Summary & Core Philosophy

The goal of this project is to establish a **universal, host-agnostic agent architecture** for QQL (Queue Query Language) that solves the cold-start retrieval problem without adding host-specific complexity or disposable glue code.

### Core Principles

1. **Separation of Concerns**:
   - **Engine Layer**: `qql-rs` (Parser, Planner, REST/gRPC/Edge Execution). Stable, high-performance core engine.
   - **Execution API**: `qql` CLI (`qql exec`, `qql explain`, `qql doctor`, `qql execute`). The single universal interface for humans and AI agents across all environments (Grok, Claude Code, Antigravity, OpenCode, Cursor, scripts).
   - **Syntax & Intent Knowledge**: `skills/qql-skill/` + [AGENT.md](file:///data/codebases/qql-rs/AGENT.md). Teaches LLMs how to parse, construct, and validate QQL statements.
   - **Memory Corpus (Knowledge)**: OKF v0.2 bundle stored in `knowledge/`. The Git-tracked, human-reviewable, version-controlled source of truth for proven query patterns and collection profiles.
   - **Agent Runtime Glue**: Host-specific plugins (`qql.ts`, MCP servers, subagent permission graphs) are treated as **disposable convenience wrappers** rather than core product APIs.

2. **Git-Portable Truth over Inverted Databases**:
   - The primary source of truth for query patterns is **OKF Markdown files on disk** in `knowledge/`.
   - Vector search indexes (`qql_memory` collection in Qdrant) are **derived artifacts** generated from the OKF bundle. If Qdrant is wiped or offline, memory remains accessible via plain text search (`index.md` / `grep`).

3. **Pattern Memory vs. Telemetry Noise**:
   - We separate **QQL Patterns** (curated, reusable templates with intent, parameters, and trust levels) from **Run Receipts** (execution logs). Memory accumulates verified, high-value retrieval knowledge rather than dumping raw log streams.

---

## 1. System Architecture

```
                      ┌──────────────────────────────────────────────┐
                      │    Host AI Agents & Developer CLI            │
                      │  (Grok / Claude / Antigravity / OpenCode)    │
                      └──────────────────────┬───────────────────────┘
                                             │
                       Universal Interface   │  Terminal Commands
                       (qql CLI)             │  (qql exec / explain)
                                             ▼
                      ┌──────────────────────────────────────────────┐
                      │             qql CLI Executable               │
                      └──────┬────────────────────────────────┬──────┘
                             │                                │
                 Execute     │                                │  Index / Search
                 Queries     ▼                                ▼  Memory Patterns
                      ┌──────────────┐                ┌──────────────────────┐
                      │ Target       │                │ Derived Memory Index │
                      │ Collections  │                │ (qql_memory in       │
                      │ (Qdrant /    │                │  Qdrant / Edge)      │
                      │  Edge)       │                └──────────▲───────────┘
                      └──────────────┘                           │
                                                                 │ Indexing Pipeline
                                                                 │ (scripts / qql CLI)
                                                      ┌──────────┴───────────┐
                                                      │ OKF Memory Bundle    │
                                                      │ (knowledge/ on disk) │
                                                      └──────────────────────┘
```

---

## 2. OKF v0.2 Knowledge Bundle Structure

The memory layer uses the Open Knowledge Format (v0.2) as defined in [okf.md](file:///data/codebases/qql-rs/okf.md).

```
knowledge/
├── index.md                      # Directory index & progressive disclosure entry point
├── log.md                        # Chronological update & verification log
├── collections/                  # Collection Profiles (schema & indexing constraints)
│   ├── berlin_airbnb.md
│   └── sec10k.md
├── patterns/                     # Verified QQL Query Patterns
│   ├── berlin/
│   │   └── Mitte_under_200.md
│   └── sec10k/
│       └── hybrid_risk_factors.md
└── runs/                         # (Optional) Execution receipts / telemetry logs
```

### 2.1 Collection Profile Concept (`knowledge/collections/berlin_airbnb.md`)

```markdown
---
type: Collection Profile
title: Berlin Airbnb Listings Collection
description: Short-term rental listings in Berlin with dense vectors and metadata.
resource: qql://berlin_airbnb
tags: [collection, real-estate, berlin]
status: stable

generated: { by: human:srimon, at: 2026-07-28T00:00:00Z }
verified: { by: process:qql-doctor, at: 2026-07-28T00:00:00Z }

vectors:
  dense: { size: 768, distance: Cosine, model: nomic-embed-text }

indexed_fields:
  - { field: price, type: float }
  - { field: neighbourhood, type: keyword }
  - { field: room_type, type: keyword }
---

# Overview

Collection containing Berlin Airbnb properties with dense text embeddings over descriptions.

# Canonical Query Example

```sql
QUERY 'cozy studio near public transit'
FROM berlin_airbnb
USING dense
FILTER price <= 100 AND neighbourhood = 'Mitte'
LIMIT 10;
```
```

### 2.2 QQL Pattern Concept (`knowledge/patterns/berlin/mitte_under_200.md`)

```markdown
---
type: QQL Pattern
title: Mitte Apartments Under €200
description: Hybrid/dense vector search for budget apartments in Berlin Mitte.
tags: [berlin_airbnb, dense, geo-filter, price-filter]
status: stable
stale_after: 2026-12-31

collection: berlin_airbnb
intent: "Find affordable accommodation in Berlin Mitte under budget"

generated: { by: grok/3.5, at: 2026-07-28T00:00:00Z }
verified:
  - { by: process:qql-exec, at: 2026-07-28T00:01:00Z }
  - { by: human:srimon, at: 2026-07-28T00:02:00Z }

sources:
  - id: collection-schema
    resource: /collections/berlin_airbnb.md
    title: Berlin Airbnb Collection Profile

parameters:
  - { name: QUERY_TEXT, type: string, required: true, description: "Semantic search text" }
  - { name: MAX_PRICE, type: number, required: true, description: "Maximum price ceiling in EUR" }
  - { name: LIMIT, type: integer, required: false, default: 10 }
---

# Pattern Template

```sql
QUERY '$QUERY_TEXT'
FROM berlin_airbnb
USING dense
WHERE price <= $MAX_PRICE AND neighbourhood = 'Mitte'
LIMIT $LIMIT;
```

# Usage Guidelines

- **When to use**: User asks for budget-constrained rentals in Mitte on `berlin_airbnb`.
- **Optimization Note**: Push `price` and `neighbourhood` filters into server-side `WHERE`/`FILTER` clause rather than post-filtering.

# Sources & Verification

Verified against schema definitions in collection profile.[^collection-schema]

[^collection-schema]: Berlin Airbnb Collection Profile
```

---

## 3. Self-Referential Querying (QQL over Memory)

To allow agents to search past memory using natural language, the `knowledge/patterns/` directory is indexed into a dedicated Qdrant collection named `qql_memory`.

### Schema of `qql_memory` Points

| Payload Field | Type | Purpose |
| :--- | :--- | :--- |
| `id` | String | Concept path (e.g., `patterns/berlin/mitte_under_200.md`) |
| `title` | String | Pattern title |
| `description` | String | Summary description |
| `collection` | String | Target collection name (`berlin_airbnb`, `sec10k`) |
| `intent` | String | Plain language intent description |
| `status` | String | Lifecycle state (`draft`, `stable`, `deprecated`) |
| `pattern_text` | String | The exact QQL query template |
| `dense` vector | Vector | Embedding of `title + description + intent` |
| `sparse` vector | Sparse Vector | BM25 sparse index over `tags` and `pattern_text` |

### Querying Memory via QQL CLI

Agents search memory before generating queries from scratch:

```bash
qql exec "QUERY HYBRID TEXT 'affordable apartment in Mitte' \
  DENSE dense SPARSE sparse FUSION RRF \
  FROM qql_memory \
  FILTER collection = 'berlin_airbnb' AND status = 'stable' \
  LIMIT 3 \
  WITH PAYLOAD INCLUDE (id, title, description, pattern_text);" --json
```

---

## 4. The Universal Agent Protocol (Skill & Agent Loop)

The memory protocol is encoded in `skills/qql-skill/SKILL.md` and [AGENT.md](file:///data/codebases/qql-rs/AGENT.md) as a standard operational procedure.

### The 6-Step Protocol

```
1. RECEIVE TASK ────► 2. SEARCH MEMORY ────► 3. EVALUATE HIT
                         (QQL on memory)        │
                                                ├──► Strong Hit: Bind Parameters
                                                ├──► Weak Hit:   Adapt Pattern
                                                └──► No Hit:     Synthesize from Syntax
                                                        │
                                                        ▼
6. RECORD MEMORY ◄─── 5. EXECUTE TARGET ◄─── 4. EXPLAIN & PLAN
   (Update OKF &         (qql exec --json)      (qql explain)
    memory index)
```

1. **Identify Intent & Collection**: Determine the targeted dataset (`berlin_airbnb`, `sec10k`) and intent.
2. **Search Memory**: Run a hybrid query against `qql_memory` via `qql exec`.
3. **Select & Adapt Strategy**:
   - **Exact Match**: Substitute parameters into the proven QQL pattern.
   - **Partial Match**: Adapt existing pattern clauses (e.g., adjust filter conditions).
   - **Cold Start**: Synthesize statement from basic QQL syntax rules in `SKILL.md`.
4. **Validate Plan**: Run `qql explain "<query>"` to ensure syntax validity, typed AST generation, and correct REST/gRPC endpoint routing.
5. **Execute Target Query**: Run `qql exec --json "<query>"`.
6. **Record & Attest**:
   - If the query introduced a novel, reusable retrieval pattern that succeeded, write a new OKF Markdown file under `knowledge/patterns/` with `status: draft` and `verified: { by: process:qql-exec }`.
   - Re-index `knowledge/` into `qql_memory`.

---

## 5. Answers to Key Architectural Questions

| Question | Architectural Resolution |
| :--- | :--- |
| **Empty vs. Seeded Memory?** | **Seeded**. We pre-populate `knowledge/` with seed patterns from `demo/` and `examples/` (`berlin_airbnb`, `sec10k`). Empty memory creates a cold-start chicken-and-egg problem. |
| **Memory Storage Location?** | **In-repo OKF bundle (`knowledge/`)**. Git-tracked, diffable, host-agnostic. The vector search index (`qql_memory`) is derived and rebuildable. |
| **Handling Staleness?** | **Schema-Fingerprint Staleness**. Collection profiles track schema fingerprints. If a collection schema changes, associated patterns are marked `status: deprecated` or `status: draft`. |
| **Role of Conformance Tests?** | **Orthogonal**. `crates/qql-conformance` validates the correctness of the parser and planner implementation. `knowledge/` stores operational retrieval playbooks for agents. |
| **OpenCode vs. Grok Alignment?** | **Universal Markdown + CLI**. OpenCode subagent configs (`~/.config/opencode/agents/`) and TS tools remain local host wrappers. Repository knowledge (`AGENT.md`, `skills/qql-skill/`, `knowledge/`) is universal across all agent harnesses. |

---

## 6. Implementation Roadmap

### Phase 1: Universal CLI & Skill Alignment
- Update `skills/qql-skill/SKILL.md` to emphasize CLI execution (`qql exec`, `qql explain`, `qql doctor`) as the canonical interface.
- Add the **Memory Protocol** section to `SKILL.md`.

### Phase 2: OKF Memory Bundle Setup & Seed Corpus
- Create the `knowledge/` directory hierarchy (`collections/`, `patterns/`, `index.md`, `log.md`).
- Populate seed collection profiles for `berlin_airbnb` and `sec10k`.
- Populate initial seed QQL patterns for dense search, filtered hybrid search, and payload scroll.

### Phase 3: Memory Indexer & Hybrid Search
- Create a lightweight memory indexing script/tool (`scripts/index_memory.rs` or `qql memory index`) that parses `knowledge/**/*.md` frontmatter and body, generates embeddings, and populates the `qql_memory` collection.
- Test self-referential search (`qql exec "QUERY ... FROM qql_memory"`).

### Phase 4: Schema Fingerprinting & Auto-Staleness
- Add collection schema hashing to `qql doctor` / collection profile generator.
- Implement automated verification checks to flag stale memory patterns when target collection schemas change.
