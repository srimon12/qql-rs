use serde_json;
use std::collections::HashMap;

use crate::executor::{ExecResponse, Executor, SetPayloadReq, UpdateVectorsReq};
use crate::filter_conv::FilterConverter;
use qql_core::ast::{self};
use qql_core::error::QqlError;

use crate::executor::helpers::{to_point_id_static, value_to_json};

impl Executor {
    pub(crate) async fn do_update_vector(
        &self,
        stmt: ast::UpdateVectorStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let point_id = to_point_id_static(&stmt.point_id)?;

        let req = UpdateVectorsReq {
            collection_name: stmt.collection.to_string(),
            point_id,
            vector: stmt.vector.clone(),
            vector_name: stmt.vector_name.map(|s| s.to_string()),
        };

        self.client.update_vectors(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "update_vector".to_string(),
            message: "Vector updated".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_update_payload(
        &self,
        stmt: ast::UpdatePayloadStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        let point_id = if let Some(ref id) = stmt.point_id {
            Some(to_point_id_static(id)?)
        } else {
            None
        };

        let payload: HashMap<String, serde_json::Value> = stmt
            .payload
            .iter()
            .map(|(k, v)| (k.to_string(), value_to_json(v)))
            .collect();

        let req = SetPayloadReq {
            collection_name: stmt.collection.to_string(),
            point_id,
            filter,
            payload,
        };

        self.client.set_payload(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "update_payload".to_string(),
            message: "Payload updated".to_string(),
            data: None,
        })
    }
}
