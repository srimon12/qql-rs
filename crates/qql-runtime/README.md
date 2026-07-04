# qql-runtime

Execution pipeline, vector embedding resolution, sparse vector tokenization, and Qdrant integration engine for QQL.

---

## Features
* **Qdrant Operations Adapter**: Wraps interactions with the official `qdrant-client` client, implementing abstract collection DDL and DML operations.
* **Semantic Embeddings Pipeline**: Resolves text-to-vector embeddings through local/cloud models.
* **Sparse Vector Tokenizer**: Implements BM25-style tokenizers for hybrid and sparse index querying.
* **Evaluation Pipeline**: Translates AST nodes into Qdrant-executable pipeline stages, handling prefetches, CTEs, and RRF/DBSF fusion.

---

## Installation

Add `qql-runtime` to your `Cargo.toml`:
```toml
[dependencies]
qql-runtime = { path = "../qql-runtime" }
```

---

## Usage

```rust
use qql_runtime::executor::Executor;
use qql_runtime::config::QqlConfig;
use qdrant_client::Qdrant; // Or custom mock client

#[tokio::main]
async fn main() {
    let config = QqlConfig::default();
    
    // Connect to Qdrant instance
    let client = qdrant_client::Qdrant::from_url("http://localhost:6334").build().unwrap();
    
    // Create the executor
    let executor = Executor::new(Box::new(client), Some(config));
    
    // Execute a QQL statement
    let response = executor.execute("QUERY 'machine learning' FROM docs LIMIT 5").await.unwrap();
    println!("Found points: {:?}", response.data);
}
```
