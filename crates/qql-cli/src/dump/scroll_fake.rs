//! Test-only in-memory `QdrantOps` for scroll pagination.
//!
//! Implements Qdrant's inclusive-offset scroll semantics over a fixed point
//! list: `offset = Some(id)` starts at `id` (not after it), which is exactly
//! the behavior the `ScrollPages` repeat-drop must tolerate.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use qql::client::{AliasAction, QdrantOps};
use qql::executor::{BackendResponse, ExecData, SearchHit};
use qql_core::error::QqlError;
use qql_plan::semantic::PlanPointId;
use qql_plan::types::ReadConsistencyParam;
use qql_plan::{
    CreateCollectionRequest, CreateIndexRequest, PlannedOperation, QueryBatchRequest,
    UpdateBatchRequest, UpdateCollectionRequest,
};
use serde_json::Value;

/// In-memory scroll backend over `points` (raw scroll JSON, ordered).
pub(crate) struct FakeScrollOps {
    points: Vec<Value>,
    /// Every scroll limit requested, in call order.
    pub(crate) limits: Mutex<Vec<u64>>,
}

impl FakeScrollOps {
    pub(crate) fn new(points: Vec<Value>) -> Self {
        Self {
            points,
            limits: Mutex::new(Vec::new()),
        }
    }
}

fn point_id(point: &Value) -> PlanPointId {
    super::json_to_plan_point_id(point.get("id").expect("fake point has an id"))
        .expect("fake point id is numeric or a string")
}

#[async_trait]
impl QdrantOps for FakeScrollOps {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn collection_exists(&self, _name: &str) -> Result<bool, QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn get_collection_info(
        &self,
        _name: &str,
    ) -> Result<qql::client::CollectionInfo, QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn create_collection(
        &self,
        _collection_name: &str,
        _req: &CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn update_collection(
        &self,
        _collection_name: &str,
        _req: &UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn create_field_index(
        &self,
        _collection_name: &str,
        _req: &CreateIndexRequest,
    ) -> Result<(), QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn delete_field_index(
        &self,
        _collection_name: &str,
        _field_name: &str,
    ) -> Result<(), QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn execute_planned(
        &self,
        op: &PlannedOperation,
    ) -> Result<BackendResponse, QqlError> {
        let PlannedOperation::Scroll { request, .. } = op else {
            unimplemented!("FakeScrollOps only implements scroll")
        };
        let limit = request.limit.unwrap_or(10);
        self.limits.lock().expect("limits lock").push(limit);
        let start = match &request.offset {
            None => 0,
            Some(offset) => self
                .points
                .iter()
                .position(|point| &point_id(point) == offset)
                .expect("offset must be a known point id"),
        };
        let hits: Vec<SearchHit> = self.points[start..]
            .iter()
            .take(limit as usize)
            .map(|point| SearchHit {
                id: point_id(point),
                score: 0.0,
                payload: point
                    .get("payload")
                    .and_then(|p| p.as_object())
                    .map(|map| {
                        map.iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect::<HashMap<_, _>>()
                    }),
                collection: None,
                vector: None,
            })
            .collect();
        Ok(BackendResponse {
            data: ExecData::Hits(hits),
            telemetry: None,
        })
    }

    async fn execute_query_batch(
        &self,
        _collection: &str,
        _batch: &QueryBatchRequest,
        _timeout: Option<u64>,
        _consistency: Option<ReadConsistencyParam>,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn execute_update_batch(
        &self,
        _collection: &str,
        _batch: &UpdateBatchRequest,
        _wait: bool,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }

    async fn change_aliases(&self, _actions: &[AliasAction]) -> Result<(), QqlError> {
        unimplemented!("FakeScrollOps only implements scroll")
    }
}
