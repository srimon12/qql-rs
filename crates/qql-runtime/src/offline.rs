use serde_json;

use qql_core::ast::{self, Value};
use qql_core::error::QqlError;
use qql_core::parser::Parser;

use crate::executor::helpers::value_to_json;
use crate::filter_conv::FilterConverter;

#[derive(serde::Serialize)]
pub struct CompiledQuery {
    pub stmt_type: &'static str,
    pub payload: serde_json::Value,
}

pub fn compile(input: &str) -> Result<CompiledQuery, QqlError> {
    let stmt = Parser::parse(input)?;
    let stmt_type = stmt_type_name(&stmt);
    let payload = compile_stmt(&stmt)?;
    Ok(CompiledQuery { stmt_type, payload })
}

fn stmt_type_name(stmt: &ast::Stmt) -> &'static str {
    match stmt {
        ast::Stmt::Query(_) => "query",
        ast::Stmt::Select(_) => "select",
        ast::Stmt::Scroll(_) => "scroll",
        ast::Stmt::Insert(_) => "insert",
        ast::Stmt::Delete(_) => "delete",
        ast::Stmt::UpdateVector(_) => "update_vector",
        ast::Stmt::UpdatePayload(_) => "update_payload",
        ast::Stmt::CreateCollection(_) => "create_collection",
        ast::Stmt::CreateIndex(_) => "create_index",
        ast::Stmt::AlterCollection(_) => "alter_collection",
        ast::Stmt::DropCollection(_) => "drop_collection",
        ast::Stmt::ShowCollections => "show_collections",
        ast::Stmt::ShowCollection(_) => "show_collection",
    }
}

fn compile_stmt(stmt: &ast::Stmt) -> Result<serde_json::Value, QqlError> {
    match stmt {
        ast::Stmt::Query(q) => compile_query(q),
        ast::Stmt::Select(s) => compile_select(s),
        ast::Stmt::Scroll(s) => compile_scroll(s),
        ast::Stmt::Insert(i) => compile_insert(i),
        ast::Stmt::Delete(d) => compile_delete(d),
        ast::Stmt::UpdateVector(u) => compile_update_vector(u),
        ast::Stmt::UpdatePayload(u) => compile_update_payload(u),
        ast::Stmt::CreateCollection(c) => compile_create_collection(c),
        ast::Stmt::CreateIndex(c) => compile_create_index(c),
        ast::Stmt::AlterCollection(a) => compile_alter_collection(a),
        ast::Stmt::DropCollection(d) => Ok(compile_drop_collection(d)),
        ast::Stmt::ShowCollections => Ok(compile_show_collections()),
        ast::Stmt::ShowCollection(c) => Ok(compile_show_collection(c)),
    }
}

fn compile_query(stmt: &ast::QueryStmt) -> Result<serde_json::Value, QqlError> {
    let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
        let converter = FilterConverter::new();
        converter.build_filter(filter)?
    } else {
        None
    };

    let query = if let Some(text) = stmt.query_text {
        let mut q = serde_json::json!({
            "nearest": { "document": { "text": text } }
        });
        if let Some(model) = stmt.model {
            q["nearest"]["document"]["model"] = serde_json::json!(model);
        }
        q
    } else if !stmt.raw_vector.is_empty() {
        serde_json::json!({ "nearest": stmt.raw_vector })
    } else if !stmt.positive_ids.is_empty() {
        let positive: Vec<serde_json::Value> =
            stmt.positive_ids.iter().map(value_to_json).collect();
        let negative: Vec<serde_json::Value> =
            stmt.negative_ids.iter().map(value_to_json).collect();
        let mut rec = serde_json::json!({ "recommend": { "positive": positive } });
        if !negative.is_empty() {
            rec["recommend"]["negative"] = serde_json::json!(negative);
        }
        rec
    } else if !stmt.context_pairs.is_empty() {
        let pairs: Vec<serde_json::Value> = stmt
            .context_pairs
            .iter()
            .map(|p| {
                serde_json::json!({
                    "positive": value_to_json(&p.positive),
                    "negative": value_to_json(&p.negative),
                })
            })
            .collect();
        serde_json::json!({ "context": { "pairs": pairs } })
    } else {
        return Err(QqlError::runtime(
            "QUERY must specify text, vector, or a search mode",
        ));
    };

    let mut body = serde_json::json!({
        "collection_name": stmt.collection,
        "query": query,
        "limit": stmt.limit,
        "offset": stmt.offset,
    });

    if !stmt.ctes.is_empty() || !stmt.prefetch_refs.is_empty() {
        let prefetch: Vec<serde_json::Value> = stmt
            .prefetch_refs
            .iter()
            .map(|pr| {
                let mut p = serde_json::json!({});
                if let Some(ref filter) = pr.filter {
                    if let Ok(Some(f)) = FilterConverter::new().build_filter(filter) {
                        p["filter"] = serde_json::to_value(f).unwrap_or(serde_json::Value::Null);
                    }
                }
                if let Some(threshold) = pr.score_threshold {
                    p["score_threshold"] = serde_json::json!(threshold);
                }
                p
            })
            .collect();
        body["prefetch"] = serde_json::json!(prefetch);
    }

    if let Some(qdrant_filter) = qdrant_filter {
        body["filter"] = serde_json::to_value(qdrant_filter).unwrap_or(serde_json::Value::Null);
    }
    if let Some(threshold) = stmt.score_threshold {
        body["score_threshold"] = serde_json::json!(threshold);
    }
    if let Some(using) = stmt.using_ {
        body["using"] = serde_json::json!(using);
    }
    if let Some(fusion) = stmt.fusion_type {
        body["fusion"] = serde_json::json!(fusion.to_uppercase());
    }
    if stmt.rerank {
        body["rerank"] = serde_json::json!({});
    }

    let has_hnsw_ef = stmt.with_clause.as_ref().is_some_and(|wc| wc.hnsw_ef > 0);
    let has_exact = stmt.with_clause.as_ref().is_some_and(|wc| wc.exact);
    if has_hnsw_ef || has_exact {
        let mut params = serde_json::Map::new();
        if let Some(ref wc) = stmt.with_clause {
            if wc.hnsw_ef > 0 {
                params.insert("hnsw_ef".to_string(), serde_json::json!(wc.hnsw_ef));
            }
            if wc.exact {
                params.insert("exact".to_string(), serde_json::json!(true));
            }
        }
        body["params"] = serde_json::Value::Object(params);
    }

    if let Some(ref sel) = stmt.with_payload {
        let mut wp = serde_json::Map::new();
        if let Some(enable) = sel.enable {
            wp.insert("enable".to_string(), serde_json::json!(enable));
        }
        if !sel.include.is_empty() {
            wp.insert("include".to_string(), serde_json::json!(sel.include));
        }
        if !sel.exclude.is_empty() {
            wp.insert("exclude".to_string(), serde_json::json!(sel.exclude));
        }
        body["with_payload"] = serde_json::Value::Object(wp);
    }
    if let Some(ref sel) = stmt.with_vectors {
        let mut wv = serde_json::Map::new();
        if let Some(enable) = sel.enable {
            wv.insert("enable".to_string(), serde_json::json!(enable));
        }
        if !sel.vectors.is_empty() {
            wv.insert("vectors".to_string(), serde_json::json!(sel.vectors));
        }
        body["with_vectors"] = serde_json::Value::Object(wv);
    }
    if let Some(lf) = stmt.lookup_from {
        body["lookup_from"] = serde_json::json!({ "collection": lf });
    }
    if let Some(group_by) = stmt.group_by {
        body["group_by"] = serde_json::json!(group_by);
        body["group_size"] = serde_json::json!(stmt.group_size.unwrap_or(1));
    }

    Ok(body)
}

fn compile_select(stmt: &ast::SelectStmt) -> Result<serde_json::Value, QqlError> {
    Ok(serde_json::json!({
        "collection_name": stmt.collection,
        "point_id": value_to_json(&stmt.point_id),
    }))
}

fn compile_scroll(stmt: &ast::ScrollStmt) -> Result<serde_json::Value, QqlError> {
    let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
        let converter = FilterConverter::new();
        converter.build_filter(filter)?
    } else {
        None
    };

    let mut body = serde_json::json!({
        "collection_name": stmt.collection,
        "limit": stmt.limit,
    });
    if let Some(qdrant_filter) = qdrant_filter {
        body["filter"] = serde_json::to_value(qdrant_filter).unwrap_or(serde_json::Value::Null);
    }
    if let Some(ref after) = stmt.after {
        body["after"] = value_to_json(after);
    }
    Ok(body)
}

fn compile_insert(stmt: &ast::InsertStmt) -> Result<serde_json::Value, QqlError> {
    let mut points = Vec::with_capacity(stmt.values_list.len());
    for row in &stmt.values_list {
        let mut payload = serde_json::Map::new();
        let mut id = None;
        let mut vectors = serde_json::Map::new();

        for (k, v) in row {
            match *k {
                "id" => id = Some(value_to_json(v)),
                "vector" | "_v" => match v {
                    Value::Dict(items) => {
                        for (nk, nv) in items {
                            vectors.insert(nk.to_string(), value_to_json(nv));
                        }
                    }
                    _ => {
                        vectors.insert("dense".to_string(), value_to_json(v));
                    }
                },
                k if k.starts_with("_v_") => {
                    let vec_name = k.strip_prefix("_v_").unwrap_or(k);
                    vectors.insert(vec_name.to_string(), value_to_json(v));
                }
                _ => {
                    payload.insert(k.to_string(), value_to_json(v));
                }
            }
        }

        let mut point = serde_json::Map::new();
        if let Some(id) = id {
            point.insert("id".to_string(), id);
        }
        point.insert("payload".to_string(), serde_json::Value::Object(payload));
        if !vectors.is_empty() {
            point.insert("vector".to_string(), serde_json::Value::Object(vectors));
        }
        points.push(serde_json::Value::Object(point));
    }

    Ok(serde_json::json!({
        "collection_name": stmt.collection,
        "points": points,
    }))
}

fn compile_delete(stmt: &ast::DeleteStmt) -> Result<serde_json::Value, QqlError> {
    let mut body = serde_json::json!({ "collection_name": stmt.collection });

    if let Some(ref point_id) = stmt.point_id {
        body["point_id"] = value_to_json(point_id);
    } else {
        let mut filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        if let Some(ref field) = stmt.field {
            if let Some(ref val) = stmt.value {
                let match_val = value_to_json(val);
                let cond = serde_json::json!({ "key": field, "match": { "value": match_val } });
                match serde_json::to_value(&filter).unwrap_or(serde_json::Value::Null) {
                    serde_json::Value::Null => {
                        filter = serde_json::from_value(serde_json::json!({ "must": [cond] })).ok();
                    }
                    existing => {
                        let existing_must =
                            existing.as_object().and_then(|o| o.get("must")).cloned();
                        let combined = serde_json::json!({ "must": [existing_must.unwrap_or(serde_json::Value::Null), cond] });
                        filter = serde_json::from_value(combined).ok();
                    }
                }
            }
        }
        body["filter"] = serde_json::to_value(&filter).unwrap_or(serde_json::Value::Null);
    }

    Ok(body)
}

fn compile_update_vector(stmt: &ast::UpdateVectorStmt) -> Result<serde_json::Value, QqlError> {
    Ok(serde_json::json!({
        "collection_name": stmt.collection,
        "point_id": value_to_json(&stmt.point_id),
        "vector": stmt.vector,
        "vector_name": stmt.vector_name,
    }))
}

fn compile_update_payload(stmt: &ast::UpdatePayloadStmt) -> Result<serde_json::Value, QqlError> {
    let filter = if let Some(ref f) = stmt.query_filter {
        let converter = FilterConverter::new();
        converter.build_filter(f)?
    } else {
        None
    };

    let payload: serde_json::Map<String, serde_json::Value> = stmt
        .payload
        .iter()
        .map(|(k, v)| (k.to_string(), value_to_json(v)))
        .collect();

    let mut body = serde_json::json!({
        "collection_name": stmt.collection,
        "payload": payload,
    });
    if let Some(ref point_id) = stmt.point_id {
        body["point_id"] = value_to_json(point_id);
    }
    if let Some(filter) = filter {
        body["filter"] = serde_json::to_value(filter).unwrap_or(serde_json::Value::Null);
    }
    Ok(body)
}

fn compile_create_collection(
    stmt: &ast::CreateCollectionStmt,
) -> Result<serde_json::Value, QqlError> {
    let mut body = serde_json::Map::new();
    body.insert(
        "collection_name".to_string(),
        serde_json::json!(stmt.collection),
    );

    if !stmt.vectors.is_empty() {
        let mut vectors_config = serde_json::Map::new();
        for v in &stmt.vectors {
            let distance_str = match v.distance {
                ast::VectorDistance::Cosine => "Cosine",
                ast::VectorDistance::Dot => "Dot",
                ast::VectorDistance::Euclid => "Euclid",
                ast::VectorDistance::Manhattan => "Manhattan",
            };
            let mut vp = serde_json::json!({ "size": v.size, "distance": distance_str });
            if let Some(ref hnsw) = v.hnsw {
                let mut hnsw_map = serde_json::Map::new();
                if let Some(m) = hnsw.m {
                    hnsw_map.insert("m".to_string(), serde_json::json!(m));
                }
                if let Some(ef) = hnsw.ef_construct {
                    hnsw_map.insert("ef_construct".to_string(), serde_json::json!(ef));
                }
                if !hnsw_map.is_empty() {
                    vp.as_object_mut().unwrap().insert(
                        "hnsw_config".to_string(),
                        serde_json::Value::Object(hnsw_map),
                    );
                }
            }
            let vec_name = if v.name.is_empty() { "dense" } else { v.name };
            vectors_config.insert(vec_name.to_string(), vp);
        }
        body.insert(
            "vectors_config".to_string(),
            serde_json::Value::Object(vectors_config),
        );
    }

    if stmt.hybrid || stmt.rerank {
        body.insert(
            "sparse_vectors_config".to_string(),
            serde_json::json!({
                "sparse": { "index": {} }
            }),
        );
    }

    if let Some(ref config) = stmt.config {
        if let Some(ref hnsw) = config.hnsw {
            body.insert(
                "hnsw_config".to_string(),
                serde_json::to_value(hnsw).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref quant) = config.quantization {
            body.insert(
                "quantization_config".to_string(),
                serde_json::to_value(quant).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref opts) = config.optimizers {
            body.insert(
                "optimizers_config".to_string(),
                serde_json::to_value(opts).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref params) = config.params {
            body.insert(
                "params".to_string(),
                serde_json::to_value(params).unwrap_or(serde_json::Value::Null),
            );
        }
    }

    Ok(serde_json::Value::Object(body))
}

fn compile_create_index(stmt: &ast::CreateIndexStmt) -> Result<serde_json::Value, QqlError> {
    Ok(serde_json::json!({
        "collection_name": stmt.collection,
        "field": stmt.field,
        "field_type": stmt.field_type,
    }))
}

fn compile_alter_collection(
    stmt: &ast::AlterCollectionStmt,
) -> Result<serde_json::Value, QqlError> {
    let mut body = serde_json::Map::new();
    body.insert(
        "collection_name".to_string(),
        serde_json::json!(stmt.collection),
    );
    if let Some(ref config) = stmt.config {
        if let Some(ref hnsw) = config.hnsw {
            body.insert(
                "hnsw_config".to_string(),
                serde_json::to_value(hnsw).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref opts) = config.optimizers {
            body.insert(
                "optimizers_config".to_string(),
                serde_json::to_value(opts).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref params) = config.params {
            body.insert(
                "params".to_string(),
                serde_json::to_value(params).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    Ok(serde_json::Value::Object(body))
}

fn compile_drop_collection(stmt: &ast::DropCollectionStmt) -> serde_json::Value {
    serde_json::json!({ "collection_name": stmt.collection })
}

fn compile_show_collections() -> serde_json::Value {
    serde_json::Value::Null
}

fn compile_show_collection(name: &str) -> serde_json::Value {
    serde_json::json!({ "collection_name": name })
}
