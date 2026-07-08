use serde_json;

use crate::executor::{DeletePointsReq, ExecResponse, Executor};
use crate::filter_conv::FilterConverter;
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

use crate::executor::helpers::to_point_id_static;

impl Executor {
    pub(crate) async fn do_delete(
        &self,
        stmt: ast::DeleteStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let mut filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        if let Some(ref field) = stmt.field {
            if let Some(ref val) = stmt.value {
                let match_val = match val {
                    Value::Str(s) => serde_json::json!(s),
                    Value::Int(i) => serde_json::json!(i),
                    Value::Float(f) => serde_json::json!(f),
                    Value::Bool(b) => serde_json::json!(b),
                    _ => {
                        return Err(QqlError::runtime(
                            "unsupported value type for delete filter",
                        ))
                    }
                };
                let cond = serde_json::json!({
                    "key": field,
                    "match": { "value": match_val }
                });

                let mut filter_json =
                    serde_json::to_value(&filter).unwrap_or(serde_json::json!({}));
                if filter_json.is_null() || !filter_json.is_object() {
                    filter_json = serde_json::json!({});
                }

                let must_arr = filter_json.get_mut("must").and_then(|m| m.as_array_mut());
                if let Some(musts) = must_arr {
                    musts.push(cond);
                } else {
                    filter_json["must"] = serde_json::json!([cond]);
                }

                filter = Some(
                    serde_json::from_value(filter_json)
                        .map_err(|e| QqlError::runtime(e.to_string()))?,
                );
            }
        }

        let point_id = if let Some(ref id) = stmt.point_id {
            Some(to_point_id_static(id)?)
        } else {
            None
        };

        let req = DeletePointsReq {
            collection_name: stmt.collection.to_string(),
            filter,
            point_id,
        };

        self.client.delete(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "delete".to_string(),
            message: "Points deleted".to_string(),
            data: None,
        })
    }
}
