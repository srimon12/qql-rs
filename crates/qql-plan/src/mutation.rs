use crate::filter::{point_id_req, top_level_filter, value_to_json};
use crate::query::lower_vector_value;
use crate::types::*;
use qql_core::ast::{
    ClearPayloadStmt, DeleteStmt, DeleteVectorStmt, EmbeddingSpec, PointSelector, PointVectors,
    Stmt, UpdatePayloadStmt, UpdateVectorStmt, UpsertPoint, UpsertStmt,
};

pub fn lower_upsert_request(stmt: &UpsertStmt) -> UpsertRequest {
    UpsertRequest {
        points: stmt.points.iter().map(lower_upsert_point).collect(),
        shard_key: stmt.shard_key.clone(),
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
            shard_key: stmt.shard_key.clone(),
        },
        PointSelector::Ids(ids) => DeleteRequest {
            points: Some(ids.iter().map(point_id_req).collect()),
            filter: None,
            shard_key: stmt.shard_key.clone(),
        },
        PointSelector::Filter(filter) => DeleteRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            shard_key: stmt.shard_key.clone(),
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
        PointSelector::Filter(filter) => (None, Some(top_level_filter(filter))),
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

pub fn lower_clear_payload_request(stmt: &ClearPayloadStmt) -> ClearPayloadRequest {
    match &stmt.selector {
        PointSelector::Id(id) => ClearPayloadRequest {
            points: Some(vec![point_id_req(id)]),
            filter: None,
        },
        PointSelector::Ids(ids) => ClearPayloadRequest {
            points: Some(ids.iter().map(point_id_req).collect()),
            filter: None,
        },
        PointSelector::Filter(filter) => ClearPayloadRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
        },
    }
}

pub fn lower_delete_vector_request(stmt: &DeleteVectorStmt) -> DeleteVectorRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeleteVectorRequest {
            points: Some(vec![point_id_req(id)]),
            filter: None,
            vector: stmt.vector_names.clone(),
        },
        PointSelector::Ids(ids) => DeleteVectorRequest {
            points: Some(ids.iter().map(point_id_req).collect()),
            filter: None,
            vector: stmt.vector_names.clone(),
        },
        PointSelector::Filter(filter) => DeleteVectorRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            vector: stmt.vector_names.clone(),
        },
    }
}

pub fn lower_scroll_request(
    limit: u64,
    filter: Option<&qql_core::ast::FilterExpr>,
    after: Option<&qql_core::ast::PointId>,
    shard_key: Option<String>,
) -> ScrollRequest {
    ScrollRequest {
        filter: filter.map(top_level_filter),
        offset: after.map(point_id_req),
        limit: Some(limit),
        with_payload: Some(PayloadSelectorReq::All(true)),
        with_vector: Some(VectorSelectorReq::All(false)),
        order_by: None,
        shard_key,
    }
}

pub fn embedding_has_wait(spec: &EmbeddingSpec) -> bool {
    match spec {
        EmbeddingSpec::Dense { .. } | EmbeddingSpec::Hybrid { .. } => true,
    }
}

/// Lower a mutation statement into a collection name + wire `UpdateOperation`.
/// Returns `None` for non-mutation statements (QUERY, DDL, SCROLL, COUNT, …).
pub fn lower_update_operation(stmt: &Stmt) -> Option<(String, UpdateOperation)> {
    match stmt {
        Stmt::Upsert(u) => Some((
            u.collection.clone(),
            UpdateOperation::Upsert {
                upsert: lower_upsert_request(u),
            },
        )),
        Stmt::Delete(d) => Some((
            d.collection.clone(),
            UpdateOperation::Delete {
                delete: lower_delete_request(d),
            },
        )),
        Stmt::UpdatePayload(u) => Some((
            u.collection.clone(),
            UpdateOperation::SetPayload {
                set_payload: lower_update_payload_request(u),
            },
        )),
        Stmt::ClearPayload(c) => Some((
            c.collection.clone(),
            UpdateOperation::ClearPayload {
                clear_payload: lower_clear_payload_request(c),
            },
        )),
        Stmt::UpdateVector(u) => Some((
            u.collection.clone(),
            UpdateOperation::UpdateVectors {
                update_vectors: lower_update_vector_request(u),
            },
        )),
        Stmt::DeleteVector(d) => Some((
            d.collection.clone(),
            UpdateOperation::DeleteVectors {
                delete_vectors: lower_delete_vector_request(d),
            },
        )),
        _ => None,
    }
}

/// Groups contiguous mutation statements by collection into `UpdateBatchRequest`s.
/// Only returns groups with 2+ operations — singles use the normal `route()` path.
/// Preserves relative order within each group.
pub fn route_update_batch(stmts: &[Stmt]) -> Vec<(String, UpdateBatchRequest)> {
    let mut groups: Vec<(String, Vec<UpdateOperation>)> = Vec::new();

    for stmt in stmts {
        let Some((collection, op)) = lower_update_operation(stmt) else {
            continue;
        };
        match groups.last_mut() {
            Some((coll, ops)) if coll == &collection => ops.push(op),
            _ => groups.push((collection, vec![op])),
        }
    }

    groups
        .into_iter()
        .filter(|(_, ops)| ops.len() > 1)
        .map(|(collection, operations)| (collection, UpdateBatchRequest { operations }))
        .collect()
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
    fn update_operation_upsert_wire_shape() {
        let s = parse_stmt("UPSERT INTO docs VALUES {id: 1, title: 'a'};");
        let (coll, op) = lower_update_operation(&s).expect("upsert is batchable");
        assert_eq!(coll, "docs");
        let json = serde_json::to_value(&op).unwrap();
        assert!(json.get("upsert").is_some(), "expected upsert key: {json}");
        assert_eq!(json["upsert"]["points"][0]["id"], 1);
    }

    #[test]
    fn update_batch_groups_same_collection() {
        let stmts = vec![
            parse_stmt("UPSERT INTO docs VALUES {id: 1, title: 'a'};"),
            parse_stmt("DELETE FROM docs WHERE id = 2;"),
            parse_stmt("UPDATE docs SET PAYLOAD = {status: 'ok'} WHERE id = 1;"),
        ];
        let batches = route_update_batch(&stmts);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].0, "docs");
        assert_eq!(batches[0].1.operations.len(), 3);
        let json = serde_json::to_value(&batches[0].1).unwrap();
        assert_eq!(json["operations"].as_array().unwrap().len(), 3);
        assert!(json["operations"][0].get("upsert").is_some());
        assert!(json["operations"][1].get("delete").is_some());
        assert!(json["operations"][2].get("set_payload").is_some());
    }

    #[test]
    fn update_batch_splits_collections() {
        let stmts = vec![
            parse_stmt("UPSERT INTO a VALUES {id: 1};"),
            parse_stmt("UPSERT INTO b VALUES {id: 2};"),
            parse_stmt("DELETE FROM a WHERE id = 1;"),
        ];
        // Contiguous grouping: [a], [b], [a] — none have len>1 alone so empty
        // Actually a has only one op per contiguous run. route_update_batch
        // groups by contiguous same-collection, so each group is size 1 → empty.
        let batches = route_update_batch(&stmts);
        assert!(batches.is_empty());

        let stmts2 = vec![
            parse_stmt("UPSERT INTO a VALUES {id: 1};"),
            parse_stmt("DELETE FROM a WHERE id = 2;"),
            parse_stmt("UPSERT INTO b VALUES {id: 3};"),
            parse_stmt("DELETE FROM b WHERE id = 4;"),
        ];
        let batches2 = route_update_batch(&stmts2);
        assert_eq!(batches2.len(), 2);
        assert_eq!(batches2[0].0, "a");
        assert_eq!(batches2[0].1.operations.len(), 2);
        assert_eq!(batches2[1].0, "b");
        assert_eq!(batches2[1].1.operations.len(), 2);
    }

    #[test]
    fn delete_by_filter() {
        let s = parse_stmt("DELETE FROM docs WHERE status = 'inactive';");
        let Stmt::Delete(ref d) = s else { panic!() };
        let req = lower_delete_request(d);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json["filter"],
            serde_json::json!({"must": [{"key": "status", "match": {"value": "inactive"}}]})
        );
    }

    #[test]
    fn update_vector() {
        let s = parse_stmt("UPDATE docs SET VECTOR dense = [3.0, 7.0] WHERE id = 'p1';");
        let Stmt::UpdateVector(ref uv) = s else {
            panic!()
        };
        let req = lower_update_vector_request(uv);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"][0]["id"], "p1");
        assert_eq!(
            json["points"][0]["vector"]["dense"],
            serde_json::json!([3.0, 7.0])
        );
    }

    #[test]
    fn update_payload() {
        let s = parse_stmt("UPDATE docs SET PAYLOAD = {status: 'active'} WHERE id = 42;");
        let Stmt::UpdatePayload(ref up) = s else {
            panic!()
        };
        let req = lower_update_payload_request(up);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["points"], serde_json::json!([42]));
    }

    #[test]
    fn scroll_request() {
        let s = parse_stmt("SCROLL FROM docs LIMIT 50;");
        let Stmt::Scroll(ref sc) = s else { panic!() };
        let req = lower_scroll_request(sc.limit, sc.filter.as_deref(), sc.after.as_ref(), None);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["limit"], 50);
    }
}
