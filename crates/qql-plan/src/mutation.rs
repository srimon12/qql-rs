use crate::filter::{lower_filter, point_id_req, value_to_json};
use crate::query::lower_vector_value;
use crate::types::*;
use qql_core::ast::{
    DeleteStmt, EmbeddingSpec, PointSelector, PointVectors, UpdatePayloadStmt, UpdateVectorStmt,
    UpsertPoint, UpsertStmt,
};

pub fn lower_upsert_request(stmt: &UpsertStmt) -> UpsertRequest {
    UpsertRequest {
        points: stmt.points.iter().map(lower_upsert_point).collect(),
    }
}

fn lower_upsert_point(point: &UpsertPoint) -> UpsertPointRequest {
    let mut req = UpsertPointRequest {
        id: point_id_req(&point.id),
        vector: None,
        payload: None,
    };
    if let Some(ref vectors) = point.vectors {
        req.vector = Some(lower_point_vectors(vectors));
    }
    if !point.payload.is_empty() {
        let mut payload = serde_json::Map::new();
        for (key, value) in &point.payload {
            payload.insert(key.clone(), value_to_json(value));
        }
        req.payload = Some(payload);
    }
    req
}

fn lower_point_vectors(vectors: &PointVectors) -> serde_json::Value {
    match vectors {
        PointVectors::Unnamed(v) => lower_vector_value(v),
        PointVectors::Named(entries) => {
            let mut obj = serde_json::Map::new();
            for (name, value) in entries {
                obj.insert(name.clone(), lower_vector_value(value));
            }
            serde_json::Value::Object(obj)
        }
    }
}

pub fn lower_delete_request(stmt: &DeleteStmt) -> DeleteRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeleteRequest {
            points: Some(vec![point_id_req(id)]),
            filter: None,
        },
        PointSelector::Ids(ids) => DeleteRequest {
            points: Some(ids.iter().map(point_id_req).collect()),
            filter: None,
        },
        PointSelector::Filter(filter) => DeleteRequest {
            points: None,
            filter: Some(lower_filter(filter)),
        },
    }
}

pub fn lower_update_vector_request(stmt: &UpdateVectorStmt) -> UpdateVectorRequest {
    let vector = lower_vector_value(&stmt.vector);
    let vector = if let Some(ref name) = stmt.vector_name {
        let mut obj = serde_json::Map::new();
        obj.insert(name.clone(), vector);
        serde_json::Value::Object(obj)
    } else {
        vector
    };
    UpdateVectorRequest {
        points: vec![UpdateVectorPoint {
            id: point_id_req(&stmt.point_id),
            vector,
        }],
    }
}

pub fn lower_update_payload_request(stmt: &UpdatePayloadStmt) -> UpdatePayloadRequest {
    let (points, filter) = match &stmt.selector {
        PointSelector::Id(id) => (Some(vec![point_id_req(id)]), None),
        PointSelector::Ids(ids) => (Some(ids.iter().map(point_id_req).collect()), None),
        PointSelector::Filter(filter) => (None, Some(lower_filter(filter))),
    };
    let mut payload = serde_json::Map::new();
    for (key, value) in &stmt.payload {
        payload.insert(key.clone(), value_to_json(value));
    }
    UpdatePayloadRequest {
        points,
        filter,
        payload,
    }
}

pub fn lower_scroll_request(
    limit: u64,
    filter: Option<&qql_core::ast::FilterExpr>,
    after: Option<&qql_core::ast::PointId>,
) -> ScrollRequest {
    ScrollRequest {
        filter: filter.map(lower_filter),
        offset: after.map(point_id_req),
        limit,
        with_payload: Some(PayloadSelectorReq::All(true)),
        with_vector: Some(VectorSelectorReq::All(false)),
    }
}

pub fn embedding_has_wait(spec: &EmbeddingSpec) -> bool {
    match spec {
        EmbeddingSpec::Dense { .. } | EmbeddingSpec::Hybrid { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::Stmt;
    use qql_core::parser::Parser;

    fn parse_stmt(s: &str) -> Stmt {
        Parser::parse(s).expect("parse failed")
    }

    #[test]
    fn upsert_simple() {
        let s = parse_stmt("UPSERT INTO docs VALUES {id: 1, title: 'hello'};");
        let Stmt::Upsert(ref u) = s else { panic!() };
        let req = lower_upsert_request(u);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"][0]["id"], 1);
        assert_eq!(json["points"][0]["payload"]["title"], "hello");
    }

    #[test]
    fn upsert_with_vector() {
        let s = parse_stmt("UPSERT INTO docs VALUES {id: 1, title: 'x', vector: [1.0, 2.0]};");
        let Stmt::Upsert(ref u) = s else { panic!() };
        let req = lower_upsert_request(u);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"][0]["vector"], serde_json::json!([1.0, 2.0]));
    }

    #[test]
    fn delete_by_ids() {
        let s = parse_stmt("DELETE FROM docs WHERE id IN (1, 2);");
        let Stmt::Delete(ref d) = s else { panic!() };
        let req = lower_delete_request(d);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"], serde_json::json!([1, 2]));
    }

    #[test]
    fn delete_by_filter() {
        let s = parse_stmt("DELETE FROM docs WHERE status = 'inactive';");
        let Stmt::Delete(ref d) = s else { panic!() };
        let req = lower_delete_request(d);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json["filter"],
            serde_json::json!({"key": "status", "match": {"value": "inactive"}})
        );
    }

    #[test]
    fn update_vector() {
        let s =
            parse_stmt("UPDATE docs SET VECTOR dense = [3.0, 7.0] WHERE id = 'p1';");
        let Stmt::UpdateVector(ref uv) = s else { panic!() };
        let req = lower_update_vector_request(uv);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"][0]["id"], "p1");
        assert_eq!(json["points"][0]["vector"]["dense"], serde_json::json!([3.0, 7.0]));
    }

    #[test]
    fn update_payload() {
        let s = parse_stmt("UPDATE docs SET PAYLOAD = {status: 'active'} WHERE id = 42;");
        let Stmt::UpdatePayload(ref up) = s else { panic!() };
        let req = lower_update_payload_request(up);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"], serde_json::json!([42]));
    }

    #[test]
    fn scroll_request() {
        let s = parse_stmt("SCROLL FROM docs LIMIT 50;");
        let Stmt::Scroll(ref sc) = s else { panic!() };
        let req = lower_scroll_request(sc.limit, sc.filter.as_deref(), sc.after.as_ref());
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["limit"], 50);
    }
}
