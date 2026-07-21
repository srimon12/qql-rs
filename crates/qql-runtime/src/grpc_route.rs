use qdrant_client::qdrant;
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
    client: &qdrant_client::Qdrant,
    route: Route,
) -> Result<serde_json::Value, QqlError> {
    match route.body {
        Some(RequestBody::Query(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = to_query_points(&req, &collection)?;
            let _resp = client
                .query(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))?;
            Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
        }
        Some(RequestBody::QueryGroups(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = to_query_groups(&req, &collection)?;
            let _resp = client
                .query_groups(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))?;
            Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
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
            let _resp = client
                .get_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))?;
            Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
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
            let _resp = client
                .scroll(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))?;
            Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
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
                .map(|(k, v)| {
                    (
                        k.clone(),
                        to_qdrant_value(serde_json::Value::String(v.to_string())),
                    )
                })
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
            client.create_collection(grpc_req).await.map_err(|e| {
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
        None => match route.method {
            qql_plan::types::Method::Get if route.path == "/collections" => {
                let _resp = client
                    .list_collections()
                    .await
                    .map_err(|e| QqlError::backend("QQL-GRPC", format!("list: {e}"), None))?;
                Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
            }
            qql_plan::types::Method::Get if route.path.starts_with("/collections/") => {
                let collection = extract_collection(&route.path)?;
                let _resp = client
                    .collection_info(qdrant::GetCollectionInfoRequest {
                        collection_name: collection,
                    })
                    .await
                    .map_err(|e| {
                        QqlError::backend("QQL-GRPC", format!("get_collection: {e}"), None)
                    })?;
                Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
            }
            qql_plan::types::Method::Delete if route.path.starts_with("/collections/") => {
                let collection = extract_collection(&route.path)?;
                client
                    .delete_collection(qdrant::DeleteCollection {
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
                "dbsf" => 1,
                _ => 0,
            };
            Variant::Fusion(val)
        }
        QueryVariant::Formula(fq) => Variant::Formula(qdrant::Formula {
            expression: None,
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
        MatchValue::Any { any } => qdrant::Match {
            match_value: Some(Mv::Keywords(qdrant::RepeatedStrings {
                strings: any
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            })),
        },
        _ => qdrant::Match { match_value: None },
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
