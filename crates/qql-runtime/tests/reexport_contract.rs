//! Compile-time proof of `qql` self-sufficiency: a custom `QdrantOps` backend
//! and the parse → inject policy flow must be writable using only `qql::`
//! paths, with no direct `qql-core` / `qql-plan` dependency in this file.

use qql::client::{CollectionInfo, QdrantOps};
use qql::executor::Executor;
use qql::{
    ComparisonOp, CreateCollectionRequest, CreateIndexRequest, Parser, PlannedOperation, QqlError,
    QueryBatchRequest, UpdateBatchRequest, UpdateCollectionRequest, Value, inject_filter,
};

/// Minimal backend used purely to prove the contract compiles from `qql` paths.
struct StubBackend;

#[async_trait::async_trait]
impl QdrantOps for StubBackend {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        Ok(vec!["stub".to_string()])
    }

    async fn collection_exists(&self, _name: &str) -> Result<bool, QqlError> {
        Ok(false)
    }

    async fn get_collection_info(&self, _name: &str) -> Result<CollectionInfo, QqlError> {
        Err(QqlError::backend(
            "QQL-BACKEND",
            "stub backend serves no schema",
            None,
        ))
    }

    async fn create_collection(
        &self,
        _collection_name: &str,
        _req: &CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }

    async fn update_collection(
        &self,
        _collection_name: &str,
        _req: &UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }

    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }

    async fn create_field_index(
        &self,
        _collection_name: &str,
        _req: &CreateIndexRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }

    async fn delete_field_index(
        &self,
        _collection_name: &str,
        _field_name: &str,
    ) -> Result<(), QqlError> {
        Ok(())
    }

    async fn execute_planned(&self, _op: &PlannedOperation) -> Result<serde_json::Value, QqlError> {
        Ok(serde_json::json!({}))
    }

    async fn execute_query_batch(
        &self,
        _collection: &str,
        _batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        Ok(Vec::new())
    }

    async fn execute_update_batch(
        &self,
        _collection: &str,
        _batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        Ok(Vec::new())
    }
}

/// The executor accepts a backend built purely from `qql::` paths.
#[test]
fn executor_accepts_qql_only_backend() {
    let _executor = Executor::new(Box::new(StubBackend), None);
}

/// The parse → inject policy flow compiles and runs from `qql` paths alone.
#[test]
fn policy_flow_from_qql_paths_only() {
    let mut stmt = Parser::parse("QUERY [0.1, 0.2] FROM docs LIMIT 5").expect("parse");
    inject_filter(
        &mut stmt,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".to_string()),
    )
    .expect("inject tenant filter");
}
