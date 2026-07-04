# QQL Syntax Reference

This document provides a comprehensive reference for the Qdrant Query Language (QQL) statement syntax and grammar.

---

## 1. Collection Management (DDL)

### Create Collection
Initializes a new collection in Qdrant with optional dense/sparse configurations, vector dimension, distance metric, HNSW, or quantization options.

```sql
-- Create a basic dense vector collection
CREATE COLLECTION docs

-- Create a hybrid collection (dense + sparse)
CREATE COLLECTION docs HYBRID

-- Create a hybrid collection with automatic late-stage reranking
CREATE COLLECTION docs HYBRID RERANK

-- Create a collection using a specific embedding model
CREATE COLLECTION docs USING MODEL 'sentence-transformers/all-MiniLM-L6-v2'

-- Create a collection with custom named vectors and distance metrics
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH HNSW (m = 0)
)
```

### Config Options
Specify performance configurations such as HNSW or Quantization at the collection or vector level.

```sql
-- Create with HNSW parameters
CREATE COLLECTION docs WITH HNSW (m = 32, ef_construct = 100)

-- Create with Quantization parameters (product, scalar, binary, turbo)
CREATE COLLECTION docs WITH QUANTIZATION (type = 'scalar', quantile = 0.95)
CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)
CREATE COLLECTION docs WITH QUANTIZATION (type = 'binary', always_ram = true)
CREATE COLLECTION docs WITH QUANTIZATION (type = 'product')
```

### Alter Collection
Dynamically modify runtime optimization and index configurations of an existing collection.

```sql
ALTER COLLECTION docs WITH VECTORS (on_disk = true)
ALTER COLLECTION docs WITH HNSW (m = 32)
ALTER COLLECTION docs WITH OPTIMIZERS (max_segment_size = 500000)
ALTER COLLECTION docs WITH PARAMS (replication_factor = 3)
ALTER COLLECTION docs WITH QUANTIZATION (type = 'scalar')
ALTER COLLECTION docs WITH QUANTIZATION (disabled = true)
```

### Drop / Show
```sql
DROP COLLECTION docs
SHOW COLLECTIONS
SHOW COLLECTION docs
```

---

## 2. Index Management

Optimize payload filtering by creating indexes on specific fields.

```sql
CREATE INDEX ON COLLECTION docs FOR category TYPE keyword
CREATE INDEX ON COLLECTION docs FOR score TYPE float
CREATE INDEX ON COLLECTION docs FOR created_at TYPE datetime

-- Advanced indexes with options
CREATE INDEX ON docs FOR tags TYPE keyword WITH (is_tenant = true, on_disk = true)
CREATE INDEX ON docs FOR content TYPE text WITH (
  tokenizer = 'word',
  min_token_len = 2,
  max_token_len = 20,
  lowercase = true,
  phrase_matching = true
)
```

---

## 3. Data Ingestion (INSERT)

Points require a unique identifier (unsigned integer or UUID string) and must provide target text for auto-embedding unless pre-computed vectors are supplied.

```sql
-- Simple insertion
INSERT INTO docs VALUES {id: 1, text: 'Qdrant is a vector database', category: 'database'}

-- Multiple points insertion
INSERT INTO docs VALUES 
  {id: '550e8400-e29b-41d4-a716-446655440000', text: 'Sentence 1'},
  {id: '550e8400-e29b-41d4-a716-446655440001', text: 'Sentence 2'}

-- Direct vector insert (bypass embedder)
INSERT INTO docs VALUES {
  id: 2,
  text: 'Custom embeddings',
  vector: {
    dense: [0.1, 0.2, 0.3],
    colbert: [[0.1, 0.2], [0.3, 0.4]]
  }
}
```

### Ingestion Routing (EMBED)
Route different payload fields to separate named vectors using the `EMBED` directive.

```sql
INSERT INTO arxiv VALUES {
  id: 'paper-1',
  text: 'The body of the paper...',
  title: 'An Analysis of Vector Retrieval'
}
EMBED text INTO dense_chunk,
      title INTO dense_title USING MODEL 'sentence-transformers/all-MiniLM-L6-v2'
```

---

## 4. Query & Search (QUERY)

Unified statement for semantic, vector, hybrid, and metadata retrieval.

### Semantic Search (Nearest)
```sql
QUERY 'vector database optimization' FROM docs LIMIT 10
QUERY 'vector database' FROM docs LIMIT 10 OFFSET 20 SCORE THRESHOLD 0.7
```

### Hybrid & Sparse Search
```sql
-- Hybrid search combining dense + sparse models
QUERY 'vector databases' FROM docs LIMIT 10 USING HYBRID

-- Custom fusion parameter tuning
QUERY 'vector databases' FROM docs LIMIT 10 USING HYBRID FUSION RRF WITH (rrf_k = 60)

-- Query specifically using sparse or dense index
QUERY 'vector databases' FROM docs LIMIT 10 USING SPARSE
```

### Recommendation and Discovery
Explore the vector space using positive and negative point references.

```sql
-- Recommend points close to id-1 but far from id-2
QUERY RECOMMEND WITH (positive = ('id-1'), negative = ('id-2')) FROM docs LIMIT 10

-- Discovery search targeting id-1 relative to context pairs
QUERY DISCOVER TARGET 'id-1' CONTEXT PAIRS (('id-2', 'id-3')) FROM docs LIMIT 10
```

### Common Table Expressions (CTEs) & Prefetches
Build complex search pipelines combining multiple semantic vectors and fusing their results.

```sql
WITH
  dense_search AS (QUERY 'vector databases' USING 'dense' LIMIT 100),
  sparse_search AS (QUERY 'vector databases' USING 'sparse' LIMIT 100)
QUERY 'vector databases' FROM docs LIMIT 10
  PREFETCH (
    dense_search WHERE category = 'tech' SCORE THRESHOLD 0.5,
    sparse_search SCORE THRESHOLD 0.3
  )
  FUSION RRF
```
