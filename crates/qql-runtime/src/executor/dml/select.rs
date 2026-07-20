use serde_json;

use crate::executor::{ExecResponse, Executor, GetPointsReq, ScrollPointsReq};
use crate::filter_conv::FilterConverter;
use qql_core::ast::{self};
use qql_core::error::QqlError;

use crate::executor::helpers::{clone_value, point_id_string, to_point_id_static};

impl Executor {
    pub(crate) async fn do_select(
        &self,
        stmt: ast::SelectStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let req = GetPointsReq {
            collection_name: stmt.collection.to_string(),
            point_id: clone_value(&stmt.point_id),
        };
        let results = self.client.get(req).await?;

        let single = results.into_iter().next();
        let data = single
            .map(|p| serde_json::to_value(&p).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null);

        Ok(ExecResponse {
            ok: true,
            operation: "select".to_string(),
            message: format!("Found {} point(s)", if data.is_null() { 0 } else { 1 }),
            data: Some(data),
        })
    }

    pub(crate) async fn do_scroll(
        &self,
        stmt: ast::ScrollStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(filter)?
        } else {
            None
        };

        let after = stmt
            .after
            .as_ref()
            .map(|v| to_point_id_static(v))
            .transpose()?;

        let req = ScrollPointsReq {
            collection_name: stmt.collection.to_string(),
            limit: stmt.limit as u64,
            filter: qdrant_filter,
            after,
        };

        let (points, next_offset) = self.client.scroll(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "scroll".to_string(),
            message: format!("Found {} point(s)", points.len()),
            data: Some(serde_json::json!({
                "points": points,
                "next_offset": next_offset.map(|p| point_id_string(&p)),
            })),
        })
    }
}
