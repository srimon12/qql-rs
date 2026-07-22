use crate::qdrant_grpc::qdrant;
use qql_core::error::QqlError;
use qql_plan::routing::{RequestBody, Route};
use qql_plan::types::{
    FilterClause, FilterCompound, FilterExpression, MatchValue, PayloadSelectorReq,
    VectorSelectorReq, WithLookupValue,
};

fn extract_collection(path: &str) -> Result<String, QqlError> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.len() >= 2
        && segments[0] == "collections"
        && segments[1] != "points"
        && !segments[1].is_empty()
    {
        Ok(segments[1].to_string())
    } else {
        Err(QqlError::execution(
            "QQL-GRPC",
            format!("cannot extract collection from path: {path}"),
            None,
        ))
    }
}

pub async fn execute_grpc_route(
    client: &crate::grpc::GrpcQdrant,
    route: Route,
) -> Result<serde_json::Value, QqlError> {
    match route.body {
        Some(RequestBody::Query(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = to_query_points(&req, &collection)?;
            let resp = client
                .query(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))?;
            Ok(serde_json::json!({
                "result": resp.result.into_iter().map(scored_point_to_json).collect::<Vec<_>>(),
                "status": "ok",
                "time": resp.time,
            }))
        }
        Some(RequestBody::QueryGroups(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = to_query_groups(&req, &collection)?;
            let resp = client
                .query_groups(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))?;
            Ok(serde_json::json!({
                "result": groups_result_to_json(resp.result.ok_or_else(|| QqlError::backend(
                    "QQL-GRPC", "missing groups result", None,
                ))?),
                "status": "ok",
                "time": resp.time,
            }))
        }
        Some(RequestBody::Points(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::GetPoints {
                collection_name: collection,
                ids: req.ids.iter().map(to_point_id).collect(),
                with_payload: req.with_payload.as_ref().map(to_payload_selector),
                with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
                ..Default::default()
            };
            let resp = client
                .get_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))?;
            Ok(serde_json::json!({
                "result": resp.result.into_iter().map(retrieved_point_to_json).collect::<Vec<_>>(),
                "status": "ok",
                "time": resp.time,
            }))
        }
        Some(RequestBody::Scroll(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::ScrollPoints {
                collection_name: collection,
                filter: req.filter.as_ref().map(to_filter),
                offset: req.offset.as_ref().map(to_point_id),
                limit: req.limit.map(|l| l as u32),
                with_payload: req.with_payload.as_ref().map(to_payload_selector),
                with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
                ..Default::default()
            };
            let resp = client
                .scroll(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))?;
            let mut obj = serde_json::Map::new();
            obj.insert("status".into(), serde_json::json!("ok"));
            obj.insert("time".into(), serde_json::json!(resp.time));
            obj.insert("result".into(), serde_json::json!({
                "points": resp.result.into_iter().map(retrieved_point_to_json).collect::<Vec<_>>()
            }));
            if let Some(offset) = resp.next_page_offset {
                obj.insert("next_page_offset".into(), point_id_to_json(&offset));
            }
            Ok(serde_json::Value::Object(obj))
        }
        Some(RequestBody::Upsert(req)) => {
            let collection = extract_collection(&route.path)?;
            let points: Vec<qdrant::PointStruct> = req
                .points
                .iter()
                .map(|p| {
                    let id = to_point_id(&p.id);
                    let vectors = p.vector.as_ref().and_then(|v| to_vectors(v.clone()));
                    let payload = p
                        .payload
                        .as_ref()
                        .map(|pl| {
                            pl.iter()
                                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                                .collect()
                        })
                        .unwrap_or_default();
                    qdrant::PointStruct {
                        id: Some(id),
                        vectors,
                        payload,
                    }
                })
                .collect();
            let grpc_req = qdrant::UpsertPoints {
                collection_name: collection,
                wait: Some(true),
                points,
                ..Default::default()
            };
            client
                .upsert_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::Delete(req)) => {
            let collection = extract_collection(&route.path)?;
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::DeletePoints {
                collection_name: collection,
                wait: Some(true),
                points: selector,
                ..Default::default()
            };
            client
                .delete_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::ClearPayload(req)) => {
            let collection = extract_collection(&route.path)?;
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::ClearPayloadPoints {
                collection_name: collection,
                wait: Some(true),
                points: selector,
                ..Default::default()
            };
            client
                .clear_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::DeleteVector(req)) => {
            let collection = extract_collection(&route.path)?;
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::DeletePointVectors {
                collection_name: collection,
                wait: Some(true),
                points_selector: selector,
                vectors: Some(qdrant::VectorsSelector {
                    names: req.vector.clone(),
                }),
                ..Default::default()
            };
            client
                .delete_vectors(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_vectors: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::UpdateVector(req)) => {
            let collection = extract_collection(&route.path)?;
            let points: Vec<qdrant::PointVectors> = req
                .points
                .iter()
                .map(|p| qdrant::PointVectors {
                    id: Some(to_point_id(&p.id)),
                    vectors: to_vectors(p.vector.clone()),
                })
                .collect();
            let grpc_req = qdrant::UpdatePointVectors {
                collection_name: collection,
                wait: Some(true),
                points,
                ..Default::default()
            };
            client
                .update_vectors(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::UpdatePayload(req)) => {
            let collection = extract_collection(&route.path)?;
            let payload_map: std::collections::HashMap<String, qdrant::Value> = req
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::SetPayloadPoints {
                collection_name: collection,
                wait: Some(true),
                points_selector: selector,
                payload: payload_map,
                ..Default::default()
            };
            client
                .set_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::CreateCollection(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::CreateCollection {
                collection_name: collection,
                vectors_config: req.vectors.as_ref().map(|v| {
                    let map = v
                        .iter()
                        .map(|(name, cfg)| {
                            let size = cfg.get("size").and_then(|s| s.as_u64()).unwrap_or(384);
                            let dist = cfg
                                .get("distance")
                                .and_then(|d| d.as_str())
                                .map(|d| match d {
                                    "Cosine" => qdrant::Distance::Cosine as i32,
                                    "Euclid" => qdrant::Distance::Euclid as i32,
                                    "Dot" => qdrant::Distance::Dot as i32,
                                    "Manhattan" => qdrant::Distance::Manhattan as i32,
                                    _ => qdrant::Distance::Cosine as i32,
                                })
                                .unwrap_or(qdrant::Distance::Cosine as i32);
                            (
                                name.clone(),
                                qdrant::VectorParams {
                                    size,
                                    distance: dist,
                                    ..Default::default()
                                },
                            )
                        })
                        .collect();
                    qdrant::VectorsConfig {
                        config: Some(qdrant::vectors_config::Config::ParamsMap(
                            qdrant::VectorParamsMap { map },
                        )),
                    }
                }),
                ..Default::default()
            };
            client.create_collection_raw(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::CreateIndex(req)) => {
            let collection = extract_collection(&route.path)?;
            let field_type = match req.field_schema.as_str() {
                "keyword" => qdrant::FieldType::Keyword as i32,
                "integer" => qdrant::FieldType::Integer as i32,
                "float" => qdrant::FieldType::Float as i32,
                "geo" => qdrant::FieldType::Geo as i32,
                "text" => qdrant::FieldType::Text as i32,
                "bool" => qdrant::FieldType::Bool as i32,
                "datetime" => qdrant::FieldType::Datetime as i32,
                "uuid" => qdrant::FieldType::Uuid as i32,
                _ => qdrant::FieldType::Keyword as i32,
            };
            let grpc_req = qdrant::CreateFieldIndexCollection {
                collection_name: collection,
                wait: Some(true),
                field_name: req.field_name.clone(),
                field_type: Some(field_type),
                ..Default::default()
            };
            client.create_field_index(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_field_index: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::Count(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::CountPoints {
                collection_name: collection,
                filter: req.filter.as_ref().map(to_filter),
                exact: req.exact,
                ..Default::default()
            };
            let resp = client
                .count_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))?;
            Ok(serde_json::json!({
                "result": {
                    "count": resp.result.map(|r| r.count).unwrap_or(0),
                },
                "status": "ok",
                "time": 0.0_f64,
            }))
        }
        None => match route.method {
            qql_plan::types::Method::Get if route.path == "/collections" => {
                let resp = client
                    .list_collections_raw()
                    .await
                    .map_err(|e| QqlError::backend("QQL-GRPC", format!("list: {e}"), None))?;
                Ok(list_collections_response_to_json(resp))
            }
            qql_plan::types::Method::Get if route.path.starts_with("/collections/") => {
                let collection = extract_collection(&route.path)?;
                let resp = client.collection_info_raw(collection).await.map_err(|e| {
                    QqlError::backend("QQL-GRPC", format!("get_collection: {e}"), None)
                })?;
                Ok(collection_info_to_json(resp))
            }
            qql_plan::types::Method::Delete if route.path.contains("/index/") => {
                // DROP INDEX: /collections/{collection}/index/{field_name}
                let segments: Vec<&str> = route.path.trim_start_matches('/').split('/').collect();
                let collection = segments
                    .get(1)
                    .ok_or_else(|| {
                        QqlError::execution("QQL-GRPC", "cannot extract collection from path", None)
                    })?
                    .to_string();
                let field_name = segments
                    .get(3)
                    .ok_or_else(|| {
                        QqlError::execution("QQL-GRPC", "cannot extract field_name from path", None)
                    })?
                    .to_string();
                client
                    .delete_field_index(qdrant::DeleteFieldIndexCollection {
                        collection_name: collection,
                        field_name,
                        wait: Some(true),
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| {
                        QqlError::backend("QQL-GRPC", format!("delete_field_index: {e}"), None)
                    })?;
                Ok(serde_json::Value::Object(Default::default()))
            }
            qql_plan::types::Method::Delete if route.path.starts_with("/collections/") => {
                let collection = extract_collection(&route.path)?;
                client
                    .delete_collection_raw(qdrant::DeleteCollection {
                        collection_name: collection,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| {
                        QqlError::backend("QQL-GRPC", format!("delete_collection: {e}"), None)
                    })?;
                Ok(serde_json::Value::Object(Default::default()))
            }
            _ => Err(QqlError::execution(
                "QQL-GRPC",
                format!("unsupported: {} {}", route.method.as_str(), route.path),
                None,
            )),
        },
    }
}

fn to_query_points(
    req: &qql_plan::types::QueryRequest,
    collection: &str,
) -> Result<qdrant::QueryPoints, QqlError> {
    Ok(qdrant::QueryPoints {
        collection_name: collection.into(),
        prefetch: req.prefetch.iter().map(to_prefetch).collect(),
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: req.filter.as_ref().map(to_filter),
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        limit: req.limit,
        offset: req.offset,
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        ..Default::default()
    })
}

fn to_query_groups(
    req: &qql_plan::types::QueryGroupsRequest,
    collection: &str,
) -> Result<qdrant::QueryPointGroups, QqlError> {
    Ok(qdrant::QueryPointGroups {
        collection_name: collection.into(),
        prefetch: req.prefetch.iter().map(to_prefetch).collect(),
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: req.filter.as_ref().map(to_filter),
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        group_by: req.group_by.clone(),
        group_size: Some(req.group_size),
        limit: Some(req.limit),
        with_lookup: req.with_lookup.as_ref().map(|wv| match wv {
            WithLookupValue::Collection(c) => qdrant::WithLookup {
                collection: c.clone(),
                ..Default::default()
            },
            WithLookupValue::Full(wl) => qdrant::WithLookup {
                collection: wl.collection.clone(),
                with_payload: wl.with_payload.as_ref().map(to_payload_selector),
                with_vectors: wl.with_vectors.as_ref().map(to_vectors_selector),
            },
        }),
        ..Default::default()
    })
}

fn to_prefetch(pf: &qql_plan::types::PrefetchRequest) -> qdrant::PrefetchQuery {
    qdrant::PrefetchQuery {
        prefetch: pf
            .prefetch
            .as_ref()
            .map(|pfs| pfs.iter().map(to_prefetch).collect())
            .unwrap_or_default(),
        query: pf.query.as_ref().and_then(|q| to_query_variant(q).ok()),
        using: pf.using.clone(),
        filter: pf.filter.as_ref().map(to_filter),
        params: pf.params.as_ref().map(to_search_params),
        score_threshold: pf.score_threshold.map(|s| s as f32),
        limit: pf.limit,
        lookup_from: pf.lookup_from.as_ref().map(|l| qdrant::LookupLocation {
            collection_name: l.collection.clone(),
            vector_name: l.vector.clone(),
            ..Default::default()
        }),
    }
}

fn to_query_variant(qv: &qql_plan::types::QueryVariant) -> Result<qdrant::Query, QqlError> {
    use qdrant::query::Variant;
    use qql_plan::types::QueryVariant;

    let variant = match qv {
        QueryVariant::Nearest(nq) => Variant::Nearest(to_vector_input(nq.nearest.clone())),
        QueryVariant::Recommend { recommend } => Variant::Recommend(qdrant::RecommendInput {
            positive: recommend
                .positive
                .iter()
                .map(|v| to_vector_input(v.clone()))
                .collect(),
            negative: recommend
                .negative
                .iter()
                .map(|v| to_vector_input(v.clone()))
                .collect(),
            strategy: recommend.strategy.as_deref().map(|s| match s {
                "average_vector" => qdrant::RecommendStrategy::AverageVector as i32,
                "best_score" => qdrant::RecommendStrategy::BestScore as i32,
                "sum_scores" => qdrant::RecommendStrategy::SumScores as i32,
                _ => qdrant::RecommendStrategy::AverageVector as i32,
            }),
        }),
        QueryVariant::Context { context } => Variant::Context(qdrant::ContextInput {
            pairs: context
                .iter()
                .map(|p| qdrant::ContextInputPair {
                    positive: Some(to_vector_input(p.positive.clone())),
                    negative: Some(to_vector_input(p.negative.clone())),
                })
                .collect(),
        }),
        QueryVariant::Discover { discover } => Variant::Discover(qdrant::DiscoverInput {
            target: Some(to_vector_input(discover.target.clone())),
            context: Some(qdrant::ContextInput {
                pairs: discover
                    .context
                    .iter()
                    .map(|p| qdrant::ContextInputPair {
                        positive: Some(to_vector_input(p.positive.clone())),
                        negative: Some(to_vector_input(p.negative.clone())),
                    })
                    .collect(),
            }),
        }),
        QueryVariant::OrderBy { order_by } => {
            let dir = order_by.direction.as_deref().map(|d| match d {
                "asc" => qdrant::Direction::Asc as i32,
                "desc" => qdrant::Direction::Desc as i32,
                _ => qdrant::Direction::Asc as i32,
            });
            Variant::OrderBy(qdrant::OrderBy {
                key: order_by.key.clone(),
                direction: dir,
                ..Default::default()
            })
        }
        QueryVariant::Sample { .. } => Variant::Sample(0),
        QueryVariant::Fusion { fusion } => {
            let val = match fusion.as_str() {
                "rrf" => 1,
                "dbsf" => 2,
                _ => 0,
            };
            Variant::Fusion(val)
        }
        QueryVariant::Rrf(_rrf_q) => Variant::Fusion(1),
        QueryVariant::Formula(fq) => Variant::Formula(qdrant::Formula {
            expression: to_formula_expression(&fq.formula),
            defaults: fq
                .defaults
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect(),
        }),
        QueryVariant::RelevanceFeedback { relevance_feedback } => {
            let feedback = relevance_feedback
                .feedback
                .iter()
                .map(|item| qdrant::FeedbackItem {
                    example: Some(to_vector_input(item.example.clone())),
                    score: item.score as f32,
                })
                .collect();
            let strategy = Some(qdrant::FeedbackStrategy {
                variant: Some(qdrant::feedback_strategy::Variant::Naive(
                    qdrant::NaiveFeedbackStrategy {
                        a: relevance_feedback.strategy.naive.a as f32,
                        b: relevance_feedback.strategy.naive.b as f32,
                        c: relevance_feedback.strategy.naive.c as f32,
                    },
                )),
            });
            Variant::RelevanceFeedback(qdrant::RelevanceFeedbackInput {
                target: Some(to_vector_input(relevance_feedback.target.clone())),
                feedback,
                strategy,
            })
        }
    };
    Ok(qdrant::Query {
        variant: Some(variant),
    })
}

fn to_vector_input(val: serde_json::Value) -> qdrant::VectorInput {
    use qdrant::vector_input::Variant;
    if let Some(n) = val.as_u64() {
        return qdrant::VectorInput {
            variant: Some(Variant::Id(qdrant::PointId {
                point_id_options: Some(qdrant::point_id::PointIdOptions::Num(n)),
            })),
        };
    }
    if let Some(s) = val.as_str() {
        return qdrant::VectorInput {
            variant: Some(Variant::Document(qdrant::Document {
                text: s.into(),
                ..Default::default()
            })),
        };
    }
    if let Some(arr) = val.as_array() {
        let data: Vec<f32> = arr
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        return qdrant::VectorInput {
            variant: Some(Variant::Dense(qdrant::DenseVector { data })),
        };
    }
    if let Some(obj) = val.as_object() {
        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
            return qdrant::VectorInput {
                variant: Some(Variant::Document(qdrant::Document {
                    text: text.into(),
                    model: obj
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                    ..Default::default()
                })),
            };
        }
    }
    qdrant::VectorInput { variant: None }
}

fn to_filter(fe: &FilterExpression) -> qdrant::Filter {
    match fe {
        FilterExpression::Compound(fc) => compound_to_filter(fc),
        FilterExpression::Single(fc) => qdrant::Filter {
            must: vec![to_condition(fc)],
            ..Default::default()
        },
    }
}

fn compound_to_filter(fc: &FilterCompound) -> qdrant::Filter {
    qdrant::Filter {
        must: fc.must.iter().map(to_condition).collect(),
        must_not: fc.must_not.iter().map(to_condition).collect(),
        should: fc.should.iter().map(to_condition).collect(),
        ..Default::default()
    }
}

fn to_condition(clause: &FilterClause) -> qdrant::Condition {
    use qdrant::condition::ConditionOneOf;
    match clause {
        FilterClause::Field(fc) => {
            let mut field = qdrant::FieldCondition {
                key: fc.key.clone(),
                ..Default::default()
            };
            if let Some(mv) = &fc.r#match {
                field.r#match = Some(to_match(mv));
            }
            if let Some(r) = &fc.range {
                field.range = Some(qdrant::Range {
                    gt: r.gt.as_ref().and_then(|v| v.as_f64()),
                    gte: r.gte.as_ref().and_then(|v| v.as_f64()),
                    lt: r.lt.as_ref().and_then(|v| v.as_f64()),
                    lte: r.lte.as_ref().and_then(|v| v.as_f64()),
                });
            }
            if let Some(b) = &fc.geo_bounding_box {
                field.geo_bounding_box = Some(qdrant::GeoBoundingBox {
                    top_left: Some(qdrant::GeoPoint {
                        lat: b.top_left.lat,
                        lon: b.top_left.lon,
                    }),
                    bottom_right: Some(qdrant::GeoPoint {
                        lat: b.bottom_right.lat,
                        lon: b.bottom_right.lon,
                    }),
                });
            }
            if let Some(r) = &fc.geo_radius {
                field.geo_radius = Some(qdrant::GeoRadius {
                    center: Some(qdrant::GeoPoint {
                        lat: r.center.lat,
                        lon: r.center.lon,
                    }),
                    radius: r.radius as f32,
                });
            }
            if let Some(vc) = &fc.values_count {
                field.values_count = Some(qdrant::ValuesCount {
                    gt: vc.gt,
                    gte: vc.gte,
                    lt: vc.lt,
                    lte: vc.lte,
                });
            }
            qdrant::Condition {
                condition_one_of: Some(ConditionOneOf::Field(field)),
            }
        }
        FilterClause::IsNull(n) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::IsNull(qdrant::IsNullCondition {
                key: n.is_null.key.clone(),
            })),
        },
        FilterClause::IsEmpty(e) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::IsEmpty(qdrant::IsEmptyCondition {
                key: e.is_empty.key.clone(),
            })),
        },
        FilterClause::HasId(h) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::HasId(qdrant::HasIdCondition {
                has_id: h.has_id.iter().map(to_point_id).collect(),
            })),
        },
        FilterClause::HasVector(v) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::HasVector(qdrant::HasVectorCondition {
                has_vector: v.has_vector.clone(),
            })),
        },
        FilterClause::Nested(n) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::Nested(qdrant::NestedCondition {
                key: n.nested.key.clone(),
                filter: Some(to_filter(&n.nested.filter)),
            })),
        },
        FilterClause::Filter(f) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::Filter(compound_to_filter(f))),
        },
    }
}

fn to_match(mv: &MatchValue) -> qdrant::Match {
    use qdrant::r#match::MatchValue as Mv;
    match mv {
        MatchValue::Value { value } => {
            if let Some(s) = value.as_str() {
                qdrant::Match {
                    match_value: Some(Mv::Keyword(s.into())),
                }
            } else if let Some(b) = value.as_bool() {
                qdrant::Match {
                    match_value: Some(Mv::Boolean(b)),
                }
            } else if let Some(n) = value.as_i64() {
                qdrant::Match {
                    match_value: Some(Mv::Integer(n)),
                }
            } else {
                qdrant::Match { match_value: None }
            }
        }
        MatchValue::Text { text } => qdrant::Match {
            match_value: Some(Mv::Text(text.clone())),
        },
        MatchValue::TextAny { text } => qdrant::Match {
            match_value: Some(Mv::TextAny(text.clone())),
        },
        MatchValue::Any { any } => qdrant::Match {
            match_value: Some(Mv::Keywords(qdrant::RepeatedStrings {
                strings: any
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            })),
        },
        MatchValue::Except { except } => qdrant::Match {
            match_value: Some(Mv::ExceptKeywords(qdrant::RepeatedStrings {
                strings: except
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            })),
        },
        MatchValue::Phrase { phrase } => qdrant::Match {
            match_value: Some(Mv::Phrase(phrase.clone())),
        },
    }
}

fn to_point_id(val: &serde_json::Value) -> qdrant::PointId {
    match val {
        serde_json::Value::Number(n) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(
                n.as_u64().unwrap_or(0),
            )),
        },
        serde_json::Value::String(s) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Uuid(s.clone())),
        },
        _ => qdrant::PointId {
            point_id_options: None,
        },
    }
}

fn to_payload_selector(ps: &PayloadSelectorReq) -> qdrant::WithPayloadSelector {
    match ps {
        PayloadSelectorReq::All(b) => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Enable(*b)),
        },
        PayloadSelectorReq::Include { include } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Include(
                qdrant::PayloadIncludeSelector {
                    fields: include.clone(),
                },
            )),
        },
        PayloadSelectorReq::Exclude { exclude } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Exclude(
                qdrant::PayloadExcludeSelector {
                    fields: exclude.clone(),
                },
            )),
        },
    }
}

fn to_vectors_selector(vs: &VectorSelectorReq) -> qdrant::WithVectorsSelector {
    match vs {
        VectorSelectorReq::All(b) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Enable(*b)),
        },
        VectorSelectorReq::Names(names) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Include(
                qdrant::VectorsSelector {
                    names: names.clone(),
                },
            )),
        },
    }
}

fn to_search_params(params: &qql_plan::types::SearchParamsRequest) -> qdrant::SearchParams {
    qdrant::SearchParams {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        indexed_only: params.indexed_only,
        ..Default::default()
    }
}

fn to_vectors(val: serde_json::Value) -> Option<qdrant::Vectors> {
    if let Some(arr) = val.as_array() {
        let data: Vec<f32> = arr
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        let vector = qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Dense(qdrant::DenseVector { data })),
            ..Default::default()
        };
        return Some(qdrant::Vectors {
            vectors_options: Some(qdrant::vectors::VectorsOptions::Vector(vector)),
        });
    }
    if let Some(obj) = val.as_object() {
        let mut map = std::collections::HashMap::new();
        for (name, v) in obj {
            if let Some(arr) = v.as_array() {
                let data: Vec<f32> = arr
                    .iter()
                    .filter_map(|f| f.as_f64().map(|x| x as f32))
                    .collect();
                map.insert(
                    name.clone(),
                    qdrant::Vector {
                        vector: Some(qdrant::vector::Vector::Dense(qdrant::DenseVector { data })),
                        ..Default::default()
                    },
                );
            }
        }
        return Some(qdrant::Vectors {
            vectors_options: Some(qdrant::vectors::VectorsOptions::Vectors(
                qdrant::NamedVectors { vectors: map },
            )),
        });
    }
    None
}

fn to_formula_expression(val: &serde_json::Value) -> Option<qdrant::Expression> {
    use qdrant::expression::Variant;
    match val {
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, val) = obj.iter().next()?;
            match key.as_str() {
                "Constant" => val.as_f64().map(|f| qdrant::Expression {
                    variant: Some(Variant::Constant(f as f32)),
                }),
                "Variable" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Variable(s.to_string())),
                }),
                "Add" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression { sum: terms })),
                    })
                }
                "Subtract" => {
                    let arr = val.as_array()?;
                    let left = to_formula_expression(&arr[0])?;
                    let right = qdrant::Expression {
                        variant: Some(Variant::Neg(Box::new(to_formula_expression(&arr[1])?))),
                    };
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression {
                            sum: vec![left, right],
                        })),
                    })
                }
                "Multiply" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression { mult: terms })),
                    })
                }
                "Divide" => {
                    let obj = val.as_object()?;
                    let left = to_formula_expression(obj.get("left")?)?;
                    let right = to_formula_expression(obj.get("right")?)?;
                    let by_zero_default = obj
                        .get("by_zero_default")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    Some(qdrant::Expression {
                        variant: Some(Variant::Div(Box::new(qdrant::DivExpression {
                            left: Some(Box::new(left)),
                            right: Some(Box::new(right)),
                            by_zero_default,
                        }))),
                    })
                }
                "Negate" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Neg(Box::new(inner))),
                    })
                }
                "Abs" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Abs(Box::new(inner))),
                    })
                }
                "Sqrt" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sqrt(Box::new(inner))),
                    })
                }
                "Log10" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Log10(Box::new(inner))),
                    })
                }
                "NaturalLog" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Ln(Box::new(inner))),
                    })
                }
                "Exp" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Exp(Box::new(inner))),
                    })
                }
                "Pow" => {
                    let arr = val.as_array()?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                            base: Some(Box::new(to_formula_expression(&arr[0])?)),
                            exponent: Some(Box::new(to_formula_expression(&arr[1])?)),
                        }))),
                    })
                }
                "GeoDistance" => {
                    let obj = val.as_object()?;
                    if let (Some(origin_obj), Some(to_val)) = (obj.get("origin"), obj.get("field"))
                    {
                        let lat = origin_obj
                            .get("latitude")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let lon = origin_obj
                            .get("longitude")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let to = to_val.as_str().unwrap_or("").to_string();
                        Some(qdrant::Expression {
                            variant: Some(Variant::GeoDistance(qdrant::GeoDistance {
                                origin: Some(qdrant::GeoPoint { lat, lon }),
                                to,
                            })),
                        })
                    } else {
                        None
                    }
                }
                "Decay" => {
                    let obj = val.as_object()?;
                    let kind_str = obj.get("kind")?.as_str()?;
                    let x = to_formula_expression(obj.get("value")?)?;
                    let target = obj.get("target").and_then(to_formula_expression);
                    let scale = obj.get("scale").and_then(|v| v.as_f64()).map(|f| f as f32);
                    let midpoint = obj
                        .get("midpoint")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    let decay = Box::new(qdrant::DecayParamsExpression {
                        x: Some(Box::new(x)),
                        target: target.map(Box::new),
                        scale,
                        midpoint,
                    });
                    let variant = match kind_str {
                        "Exponential" => Variant::ExpDecay(decay),
                        "Gaussian" => Variant::GaussDecay(decay),
                        "Linear" => Variant::LinDecay(decay),
                        _ => return None,
                    };
                    Some(qdrant::Expression {
                        variant: Some(variant),
                    })
                }
                "Case" => {
                    let obj = val.as_object()?;
                    let condition = obj.get("condition")?;
                    let then_val = to_formula_expression(obj.get("then_value")?)?;
                    let else_val = to_formula_expression(
                        obj.get("else_value").unwrap_or(&serde_json::Value::Null),
                    )?;
                    let cond_expr: qdrant::Expression = to_condition_from_json(condition)
                        .map(|c| qdrant::Expression {
                            variant: Some(Variant::Condition(c)),
                        })
                        .unwrap_or_else(|| qdrant::Expression {
                            variant: Some(Variant::Constant(1.0)),
                        });
                    let one = qdrant::Expression {
                        variant: Some(Variant::Constant(1.0)),
                    };
                    let neg_cond = qdrant::Expression {
                        variant: Some(Variant::Neg(Box::new(cond_expr.clone()))),
                    };
                    let one_minus_cond = qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression {
                            sum: vec![one, neg_cond],
                        })),
                    };
                    let then_branch = qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression {
                            mult: vec![cond_expr, then_val],
                        })),
                    };
                    let else_branch = qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression {
                            mult: vec![one_minus_cond, else_val],
                        })),
                    };
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression {
                            sum: vec![then_branch, else_branch],
                        })),
                    })
                }
                "Match" => None,
                "DateTime" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Datetime(s.to_string())),
                }),
                "DateTimeField" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::DatetimeKey(s.to_string())),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn to_condition_from_json(val: &serde_json::Value) -> Option<qdrant::Condition> {
    match val {
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, inner) = obj.iter().next()?;
            match key.as_str() {
                "And" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Or" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                should: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Not" => {
                    let inner_cond = to_condition_from_json(inner)?;
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must_not: vec![inner_cond],
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Compare" => {
                    let obj = inner.as_object()?;
                    let field = obj.get("field")?.as_str()?;
                    let op = obj.get("op")?.as_str()?;
                    let value = obj.get("value")?;
                    let range = match op {
                        "Eq" => qdrant::Range {
                            gte: value.as_f64(),
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        "Gt" => qdrant::Range {
                            gt: value.as_f64(),
                            ..Default::default()
                        },
                        "Gte" => qdrant::Range {
                            gte: value.as_f64(),
                            ..Default::default()
                        },
                        "Lt" => qdrant::Range {
                            lt: value.as_f64(),
                            ..Default::default()
                        },
                        "Lte" => qdrant::Range {
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        _ => return None,
                    };
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Field(
                            qdrant::FieldCondition {
                                key: field.to_string(),
                                range: Some(range),
                                ..Default::default()
                            },
                        )),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn to_qdrant_value(val: serde_json::Value) -> qdrant::Value {
    use qdrant::value::Kind;
    match val {
        serde_json::Value::Null => qdrant::Value { kind: None },
        serde_json::Value::Bool(b) => qdrant::Value {
            kind: Some(Kind::BoolValue(b)),
        },
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                qdrant::Value {
                    kind: Some(Kind::IntegerValue(i)),
                }
            } else {
                qdrant::Value {
                    kind: Some(Kind::DoubleValue(n.as_f64().unwrap_or(0.0))),
                }
            }
        }
        serde_json::Value::String(s) => qdrant::Value {
            kind: Some(Kind::StringValue(s)),
        },
        serde_json::Value::Array(arr) => qdrant::Value {
            kind: Some(Kind::ListValue(qdrant::ListValue {
                values: arr.into_iter().map(to_qdrant_value).collect(),
            })),
        },
        serde_json::Value::Object(obj) => {
            let fields = obj
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect();
            qdrant::Value {
                kind: Some(Kind::StructValue(qdrant::Struct { fields })),
            }
        }
    }
}

// ── Proto response → JSON conversion ─────────────────────────────

fn point_id_to_json(id: &qdrant::PointId) -> serde_json::Value {
    match &id.point_id_options {
        Some(qdrant::point_id::PointIdOptions::Num(n)) => serde_json::json!(*n),
        Some(qdrant::point_id::PointIdOptions::Uuid(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

fn group_id_to_json(id: &qdrant::GroupId) -> serde_json::Value {
    match &id.kind {
        Some(qdrant::group_id::Kind::UnsignedValue(n)) => serde_json::json!(*n),
        Some(qdrant::group_id::Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(qdrant::group_id::Kind::StringValue(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

fn qdrant_value_to_json(v: &qdrant::Value) -> serde_json::Value {
    use qdrant::value::Kind;
    match &v.kind {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::DoubleValue(d)) => serde_json::json!(*d),
        Some(Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(Kind::StringValue(s)) => serde_json::json!(s),
        Some(Kind::BoolValue(b)) => serde_json::json!(*b),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(qdrant_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => serde_json::Value::Object(
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), qdrant_value_to_json(v)))
                .collect(),
        ),
    }
}

fn vector_output_to_json(vo: &qdrant::VectorOutput) -> serde_json::Value {
    use qdrant::vector_output;
    match &vo.vector {
        Some(vector_output::Vector::Dense(d)) => {
            serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
        }
        Some(vector_output::Vector::Sparse(s)) => serde_json::json!({
            "indices": s.indices,
            "values": s.values,
        }),
        Some(vector_output::Vector::MultiDense(m)) => serde_json::Value::Array(
            m.vectors
                .iter()
                .map(|d| {
                    serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
                })
                .collect(),
        ),
        None => serde_json::Value::Null,
    }
}

fn vectors_output_to_json(v: &qdrant::VectorsOutput) -> serde_json::Value {
    use qdrant::vectors_output::VectorsOptions;
    match &v.vectors_options {
        Some(VectorsOptions::Vector(vo)) => vector_output_to_json(vo),
        Some(VectorsOptions::Vectors(named)) => {
            let mut map = serde_json::Map::new();
            for (name, vec) in &named.vectors {
                map.insert(name.clone(), vector_output_to_json(vec));
            }
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}

fn scored_point_to_json(p: qdrant::ScoredPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("score".into(), serde_json::json!(p.score));
    obj.insert("payload".into(), payload);
    if p.version != 0 {
        obj.insert("version".into(), serde_json::json!(p.version));
    }
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

fn retrieved_point_to_json(p: qdrant::RetrievedPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("payload".into(), payload);
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

fn groups_result_to_json(r: qdrant::GroupsResult) -> serde_json::Value {
    serde_json::json!({
        "groups": r.groups.into_iter().map(point_group_to_json).collect::<Vec<_>>(),
    })
}

fn point_group_to_json(g: qdrant::PointGroup) -> serde_json::Value {
    let hits: Vec<_> = g.hits.into_iter().map(scored_point_to_json).collect();
    let id =
        g.id.as_ref()
            .map_or(serde_json::Value::Null, group_id_to_json);
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("hits".into(), serde_json::json!(hits));
    if let Some(lookup) = g.lookup {
        obj.insert("lookup".into(), retrieved_point_to_json(lookup));
    }
    serde_json::Value::Object(obj)
}

fn list_collections_response_to_json(resp: qdrant::ListCollectionsResponse) -> serde_json::Value {
    serde_json::json!({
        "result": {
            "collections": resp.collections.into_iter()
                .map(|c| serde_json::json!({"name": c.name}))
                .collect::<Vec<_>>(),
        },
        "status": "ok",
        "time": resp.time,
    })
}

fn collection_info_to_json(resp: qdrant::GetCollectionInfoResponse) -> serde_json::Value {
    let info = resp.result.map(|info| {
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), serde_json::json!(info.status));
        if let Some(os) = info.optimizer_status {
            obj.insert("optimizer_status".into(), serde_json::json!(os.ok));
        }
        obj.insert(
            "segments_count".into(),
            serde_json::json!(info.segments_count),
        );
        if let Some(pc) = info.points_count {
            obj.insert("points_count".into(), serde_json::json!(pc));
        }
        if let Some(ivc) = info.indexed_vectors_count {
            obj.insert("indexed_vectors_count".into(), serde_json::json!(ivc));
        }
        if let Some(cfg) = info.config {
            obj.insert("config".into(), collection_config_to_json(&cfg));
        }
        if !info.payload_schema.is_empty() {
            let schema: serde_json::Map<_, _> = info
                .payload_schema
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "data_type": v.data_type,
                            "points": v.points,
                        }),
                    )
                })
                .collect();
            obj.insert("payload_schema".into(), serde_json::Value::Object(schema));
        }
        serde_json::Value::Object(obj)
    });
    serde_json::json!({
        "result": info,
        "status": "ok",
        "time": resp.time,
    })
}

fn collection_config_to_json(c: &qdrant::CollectionConfig) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(params) = &c.params {
        let mut p = serde_json::Map::new();
        p.insert(
            "shard_number".into(),
            serde_json::json!(params.shard_number),
        );
        p.insert(
            "on_disk_payload".into(),
            serde_json::json!(params.on_disk_payload),
        );
        if let Some(vc) = &params.vectors_config {
            p.insert("vectors".into(), vectors_config_to_json(vc));
        }
        if let Some(rf) = params.replication_factor {
            p.insert("replication_factor".into(), serde_json::json!(rf));
        }
        if let Some(wcf) = params.write_consistency_factor {
            p.insert("write_consistency_factor".into(), serde_json::json!(wcf));
        }
        if let Some(rff) = params.read_fan_out_factor {
            p.insert("read_fan_out_factor".into(), serde_json::json!(rff));
        }
        if let Some(svc) = &params.sparse_vectors_config {
            let map: serde_json::Map<_, _> = svc
                .map
                .iter()
                .map(|(k, v)| {
                    let mut entry = serde_json::Map::new();
                    if let Some(sidx) = &v.index {
                        entry.insert(
                            "index".into(),
                            serde_json::json!({
                                "on_disk": sidx.on_disk,
                            }),
                        );
                    }
                    (k.clone(), serde_json::Value::Object(entry))
                })
                .collect();
            p.insert("sparse_vectors".into(), serde_json::Value::Object(map));
        }
        obj.insert("params".into(), serde_json::Value::Object(p));
    }
    if let Some(hnsw) = &c.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(opt) = &c.optimizer_config {
        let max_threads = opt
            .max_optimization_threads
            .as_ref()
            .map(|m| match &m.variant {
                Some(qdrant::max_optimization_threads::Variant::Value(n)) => {
                    serde_json::json!(*n)
                }
                Some(qdrant::max_optimization_threads::Variant::Setting(_)) => {
                    serde_json::json!("auto")
                }
                None => serde_json::Value::Null,
            });
        obj.insert(
            "optimizer_config".into(),
            serde_json::json!({
                "deleted_threshold": opt.deleted_threshold,
                "vacuum_min_vector_number": opt.vacuum_min_vector_number,
                "default_segment_number": opt.default_segment_number,
                "max_segment_size": opt.max_segment_size,
                "memmap_threshold": opt.memmap_threshold,
                "indexing_threshold": opt.indexing_threshold,
                "flush_interval_sec": opt.flush_interval_sec,
                "max_optimization_threads": max_threads,
            }),
        );
    }
    if let Some(wal) = &c.wal_config {
        obj.insert(
            "wal_config".into(),
            serde_json::json!({
                "wal_capacity_mb": wal.wal_capacity_mb,
                "wal_segments_ahead": wal.wal_segments_ahead,
            }),
        );
    }
    if let Some(qc) = &c.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_config_to_json(qc),
        );
    }
    if let Some(sm) = &c.strict_mode_config {
        obj.insert(
            "strict_mode_config".into(),
            serde_json::json!({
                "enabled": sm.enabled,
                "max_collection_vector_size_bytes": sm.max_collection_vector_size_bytes,
                "read_rate_limit": sm.read_rate_limit,
                "write_rate_limit": sm.write_rate_limit,
                "max_query_limit": sm.max_query_limit,
            }),
        );
    }
    serde_json::Value::Object(obj)
}

fn quantization_config_to_json(qc: &qdrant::QuantizationConfig) -> serde_json::Value {
    use qdrant::quantization_config::Quantization;
    let mut obj = serde_json::Map::new();
    match &qc.quantization {
        Some(Quantization::Scalar(s)) => {
            obj.insert(
                "scalar".into(),
                serde_json::json!({
                    "r#type": s.r#type,
                    "quantile": s.quantile,
                    "always_ram": s.always_ram,
                }),
            );
        }
        Some(Quantization::Product(p)) => {
            obj.insert(
                "product".into(),
                serde_json::json!({
                    "compression": p.compression,
                    "always_ram": p.always_ram,
                }),
            );
        }
        Some(Quantization::Binary(b)) => {
            obj.insert(
                "binary".into(),
                serde_json::json!({
                    "always_ram": b.always_ram,
                }),
            );
        }
        Some(Quantization::Turboquant(_)) => {}
        None => {}
    }
    serde_json::Value::Object(obj)
}

fn vectors_config_to_json(vc: &qdrant::VectorsConfig) -> serde_json::Value {
    use qdrant::vectors_config::Config;
    match &vc.config {
        Some(Config::Params(p)) => vector_params_to_json(p),
        Some(Config::ParamsMap(pm)) => {
            let map: serde_json::Map<_, _> = pm
                .map
                .iter()
                .map(|(k, v)| (k.clone(), vector_params_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::json!({}),
    }
}

fn vector_params_to_json(vp: &qdrant::VectorParams) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("size".into(), serde_json::json!(vp.size));
    obj.insert(
        "distance".into(),
        serde_json::json!(distance_to_str(vp.distance)),
    );
    if let Some(od) = vp.on_disk {
        obj.insert("on_disk".into(), serde_json::json!(od));
    }
    if let Some(hnsw) = &vp.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(qc) = &vp.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_config_to_json(qc),
        );
    }
    if let Some(mv) = &vp.multivector_config {
        obj.insert(
            "multivector_config".into(),
            serde_json::json!({
                "comparator": multivec_comp_to_str(mv.comparator),
            }),
        );
    }
    serde_json::Value::Object(obj)
}

fn distance_to_str(d: i32) -> &'static str {
    match d {
        1 => "Cosine",
        2 => "Euclid",
        3 => "Dot",
        4 => "Manhattan",
        _ => "UnknownDistance",
    }
}

fn multivec_comp_to_str(c: i32) -> &'static str {
    match c {
        0 => "MaxSim",
        _ => "MaxSim",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;
    use qql_plan::routing::route;

    #[test]
    fn test_grpc_route_conversion_all_statements() {
        let statements = [
            "QUERY 'search' FROM docs USING dense LIMIT 10;",
            "QUERY POINTS (1, 2, 'uuid-str') FROM docs WITH PAYLOAD INCLUDE ('title');",
            "SCROLL FROM docs WHERE status = 'active' LIMIT 50;",
            "UPSERT INTO docs VALUES {id: 1, text: 'hello', category: 'tech'} USING DENSE MODEL 'm';",
            "DELETE FROM docs WHERE category = 'old';",
            "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
            "UPDATE docs SET PAYLOAD = {status: 'ok'} WHERE id = 1;",
            "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE);",
            "ALTER COLLECTION docs WITH HNSW (m = 16);",
            "DROP COLLECTION docs;",
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
            "SHOW COLLECTIONS;",
            "SHOW COLLECTION docs;",
        ];

        for stmt_str in statements {
            let stmt = Parser::parse(stmt_str)
                .unwrap_or_else(|e| panic!("parse failed for {stmt_str}: {e}"));
            let r = route(&stmt);
            match &r.body {
                Some(RequestBody::Query(req)) => {
                    let grpc_req = to_query_points(req, "docs");
                    assert!(
                        grpc_req.is_ok(),
                        "to_query_points failed for {stmt_str}: {:?}",
                        grpc_req.err()
                    );
                }
                Some(RequestBody::QueryGroups(req)) => {
                    let grpc_req = to_query_groups(req, "docs");
                    assert!(
                        grpc_req.is_ok(),
                        "to_query_groups failed for {stmt_str}: {:?}",
                        grpc_req.err()
                    );
                }
                Some(RequestBody::Points(req)) => {
                    assert_eq!(req.ids.len(), 3);
                }
                Some(RequestBody::Scroll(req)) => {
                    assert!(req.filter.is_some());
                }
                Some(RequestBody::Upsert(req)) => {
                    assert_eq!(req.points.len(), 1);
                }
                Some(RequestBody::Delete(req)) => {
                    assert!(req.filter.is_some());
                }
                Some(RequestBody::UpdateVector(_)) => {}
                Some(RequestBody::UpdatePayload(_)) => {}
                Some(RequestBody::CreateCollection(req)) => {
                    assert!(req.vectors.is_some() || req.hnsw_config.is_some());
                }
                Some(RequestBody::CreateIndex(req)) => {
                    assert_eq!(req.field_name, "title");
                }
                None => {}
            }
        }
    }
}
