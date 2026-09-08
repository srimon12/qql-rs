# Collection Migration & Snapshot Strategy Guide

Operator docs: [QUICKSTART.md](QUICKSTART.md) · [EXAMPLES.md](EXAMPLES.md).

Comprehensive architectural guide comparing **Qdrant Native Snapshots** and **QQL Logical Streaming Migration (`qql migrate`)**, informed by modern Qdrant capabilities (`v1.15` through `v1.19.1`).

---

## 1. Modern Qdrant Storage Architecture (v1.15 – v1.19.1)

Understanding modern Qdrant storage internals is essential for choosing between native snapshots and logical streaming migration.

### Release Evolution Matrix

| Version | Milestone | Impact on Snapshots & Migration |
|---|---|---|
| **v1.15** | **Gridstore Engine Finalized**<br>RocksDB fully removed; 1.5-bit & 2-bit quantization; HNSW healing. | Eliminates LSM compaction latency spikes. Pre-v1.15 binary snapshots cannot be directly mounted on v1.15+ without intermediate startup migrations. |
| **v1.16** | **Tiered Multitenancy & ACORN**<br>Dynamic shard promotion; weak-filter graph search; inline HNSW storage. | Tenant payload partitioning becomes first-class. Large tenants can be isolated to dedicated shards. |
| **v1.17** | **Optimization Monitoring API**<br>`GET /collections/{c}/optimizations`; gRPC vector serialization updates. | Segment indexing and merges can be tracked in real-time with granular progress percentages. |
| **v1.18** | **Dynamic Named Vectors**<br>Add/drop named vectors without re-ingesting collections. | Partial schema evolution supported without recreating the collection. |
| **v1.19.0** | **TurboQuant Datatype (`turbo4`) & Memory Tiers**<br>Native 4-bit storage without full f32 copies (9x compression); unified `pinned`/`cached`/`cold` memory tiers; per-tenant BM25 IDF. | New vector datatypes and memory tier models not representable in older collections. |
| **v1.19.1** | **SIMD TurboQuant & Replica Hardening**<br>Prefetching memory bandwidth optimizations; crash-safe replica-state consensus. | High-performance quantized vector scoring at scale. |

---

## 2. Fundamental Architectural Differences

```
┌────────────────────────────────────────────────────────────────────────┐
│ Qdrant Native Snapshot                                                 │
│                                                                        │
│  Source Cluster (Disk)                 Target Cluster (Disk)           │
│  [ Gridstore + HNSW + Quant ] ──tar──► [ Direct mmap into storage ]   │
│                                                                        │
│  • Byte-level segment copy                                             │
│  • O(1) CPU: HNSW graph is NOT rebuilt                                 │
│  • Instant query readiness                                             │
│  • Strictly locked to identical schema, shard count, & minor version  │
└────────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────────┐
│ QQL Logical Migration (qql migrate)                                    │
│                                                                        │
│  Source Cluster (Any)                  Target Cluster (Any)            │
│  [ Points / Vectors ] ───gRPC Stream──► [ Fresh Ingestion + Optimize ]  │
│                                                                        │
│  • Universal transport format (declarative QQL)                        │
│  • O(N log N) CPU: Target rebuilds HNSW graph fresh                    │
│  • In-flight transformation: Reshard, Quantize (Scalar/Binary/Turbo4)  │
│  • Cross-minor version compatibility (e.g. 1.15 ➔ 1.19.1)              │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Decision Matrix: When to Use What

| Scenario | Recommended Approach | Technical Rationale |
|---|:---:|---|
| **Disaster Recovery / Backup** | **Native Snapshots** | Restores in minutes via disk I/O; zero CPU penalty; exact HNSW graph recall preserved. |
| **Identical Version Clone** (`1.19.0` → `1.19.1`) | **Native Snapshots** | No schema changes needed. Rebuilding HNSW via logical migration would waste hours of CPU. |
| **Multi-Minor Version Leap** (`1.15` → `1.19.1`) | **`qql migrate`** | Qdrant storage compatibility only spans one minor version. Snapshots require 4 intermediate rolling upgrades; `qql migrate` leaps in one pass. |
| **Topology Resharding** (e.g. 2 shards → 12 shards) | **`qql migrate`** | Shard counts are immutable in Qdrant collections. Snapshots cannot alter shard count; `qql migrate` re-hashes points across shards in flight. |
| **Tenant Migration to Custom Shards** | **`qql migrate`** | Supports `--shard-key-field <tenant_id>` to transform auto-sharded data into tenant-isolated custom shards (`sharding_method = 'custom'`). |
| **In-Flight Vector Compression** (f32 → `turbo4` / `scalar`) | **`qql migrate`** | Rewrites the vector specification on `CREATE COLLECTION` to shrink memory footprint by up to 75–90% on the destination cluster. |
| **Filtered Data Extraction** (`--where "city = 'berlin'"`) | **`qql migrate`** | Snapshots are all-or-nothing per node. `qql migrate` exports precise logical subsets. |
| **Massive Collections (>50M vectors) on Limited Hardware** | **Native Snapshots** | Logical migration requires **~2x RAM and disk** during target HNSW graph construction. Memory-constrained targets risk OOM. |
| **Live Database with Continuous Writes** | **Native Snapshots** | Snapshots provide an atomic point-in-time cut. Multi-hour logical streaming without CDC will suffer from write drift. |

---

## 4. Operational Comparison

| Dimension | Qdrant Native Snapshot | QQL Logical Migration (`qql migrate`) |
|---|---|---|
| **Transfer Unit** | Raw Gridstore segments + WAL + HNSW graph files | Point IDs, payload JSON, and raw vector arrays |
| **Target CPU Load** | Negligible ($O(1)$ disk unpack & mmap) | Heavy ($O(N \log N)$ HNSW graph build & quantizer training) |
| **Target RAM Overhead** | Baseline operational memory | **~2x memory** during unindexed buffering and graph indexing |
| **Downtime / Search Readiness** | Instant (searchable immediately upon untar) | Delayed (search during index build is brute-force until optimizer runs) |
| **Network Topology** | Direct peer-to-peer / S3 / cluster internal (10+ Gbps) | Dual-hop through client runner (Source ➔ Runner ➔ Target) |
| **Schema Mutability** | Completely immutable | Fully mutable (change dimensions, distance, quantization, shards) |
| **Storage Fragmentation** | Replicated as-is (including dead point tombstones) | Completely eliminated (acts as a full vacuum & defragmentation) |

---

## 5. Best Practices for `qql migrate`

When using `qql migrate`, adhere to the following production rules:

1. **Ensure Adequate Target Hardware**:
   Confirm the destination cluster has at least **2x the RAM** of the source dataset to accommodate segment buffering and concurrent HNSW building.
2. **Leverage Fast-Bulk Protocol (`--fast-bulk`)**:
   Keep `--fast-bulk` enabled (default) so `indexing_threshold` is raised during ingest. This prevents the optimizer from repeatedly building small HNSW graphs mid-stream.
3. **Run from a Network-Adjacent Host**:
   Execute `qql migrate` from a machine located in the same cloud region / VPC as the clusters to minimize dual-hop latency and eliminate egress costs.
4. **Use Checkpoints for Large Datasets**:
   Ensure `--checkpoint` is configured on durable storage. If a network interruption occurs, re-running with `--resume` picks up from the last committed window without re-upserting earlier points.
5. **Execute Exact Verification (`--verify`)**:
   Always verify final counts before cutting over client traffic.
