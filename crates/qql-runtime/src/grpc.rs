//! gRPC transport integration backed by the official `qdrant-client` crate.

use async_trait::async_trait;
use qdrant_client::Qdrant;
use std::collections::HashMap;

use crate::client::{
    CollectionInfo, CountPointsReq, CreateCollectionReq, CreateFieldIndexReq, DeletePointsReq,
    GetPointsReq, PointGroup, QdrantAdminOps, QdrantCoreOps, RetrievedPoint, ScoredPoint,
    ScrollPointsReq, SetPayloadReq, UpdateVectorsReq, UpsertPointsReq,
};
use crate::pipeline::{PointId, QueryPointsGroupsRequest, QueryPointsRequest};
use qql_core::error::QqlError;

/// Owns, or reuses, an official Qdrant gRPC client.
pub struct GrpcQdrant {
    client: Qdrant,
}

impl GrpcQdrant {
    pub fn from_client(client: Qdrant) -> Self {
        Self { client }
    }

    pub fn from_url(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        let mut builder = Qdrant::from_url(url);
        if let Some(api_key) = api_key {
            builder = builder.api_key(api_key);
        }
        let client = builder.build().map_err(|error| {
            QqlError::runtime(format!("failed to build Qdrant gRPC client: {error}"))
        })?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &Qdrant {
        &self.client
    }
}

fn to_grpc_point_id(id: PointId) -> qdrant_client::qdrant::PointId {
    match id {
        PointId::Num(num) => qdrant_client::qdrant::PointId {
            point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(num)),
        },
        PointId::Uuid(uuid) => qdrant_client::qdrant::PointId {
            point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(uuid)),
        },
    }
}

fn from_grpc_point_id(id: qdrant_client::qdrant::PointId) -> Result<PointId, QqlError> {
    match id.point_id_options {
        Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(num)) => Ok(PointId::Num(num)),
        Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(uuid)) => {
            Ok(PointId::Uuid(uuid))
        }
        None => Err(QqlError::runtime("gRPC point ID is empty")),
    }
}

fn parse_json_filter(val: &serde_json::Value) -> Result<qdrant_client::qdrant::Filter, QqlError> {
    let mut filter = qdrant_client::qdrant::Filter::default();
    if let Some(obj) = val.as_object() {
        if let Some(must) = obj.get("must") {
            if let Some(arr) = must.as_array() {
                for item in arr {
                    filter.must.push(parse_json_condition(item)?);
                }
            }
        }
        if let Some(should) = obj.get("should") {
            if let Some(arr) = should.as_array() {
                for item in arr {
                    filter.should.push(parse_json_condition(item)?);
                }
            }
        }
        if let Some(must_not) = obj.get("must_not") {
            if let Some(arr) = must_not.as_array() {
                for item in arr {
                    filter.must_not.push(parse_json_condition(item)?);
                }
            }
        }
    }
    Ok(filter)
}

fn parse_json_condition(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::Condition, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("condition must be a JSON object"))?;

    if let Some(is_null) = obj.get("is_null") {
        let key = is_null
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Ok(qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::IsNull(
                qdrant_client::qdrant::IsNullCondition { key },
            )),
        });
    }

    if let Some(is_empty) = obj.get("is_empty") {
        let key = is_empty
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Ok(qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::IsEmpty(
                qdrant_client::qdrant::IsEmptyCondition { key },
            )),
        });
    }

    if let Some(has_id) = obj.get("has_id") {
        if let Some(arr) = has_id.as_array() {
            let ids = arr
                .iter()
                .map(|v| {
                    let id = if let Some(n) = v.as_u64() {
                        PointId::Num(n)
                    } else {
                        PointId::Uuid(v.as_str().unwrap_or_default().to_string())
                    };
                    to_grpc_point_id(id)
                })
                .collect();
            return Ok(qdrant_client::qdrant::Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::HasId(
                    qdrant_client::qdrant::HasIdCondition { has_id: ids },
                )),
            });
        }
    }

    if let Some(has_vector) = obj.get("has_vector") {
        let name = has_vector.as_str().unwrap_or_default().to_string();
        return Ok(qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::HasVector(
                qdrant_client::qdrant::HasVectorCondition { has_vector: name },
            )),
        });
    }

    if let Some(nested) = obj.get("nested") {
        let key = nested
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let inner_filter = nested.get("filter").map(parse_json_filter).transpose()?;
        return Ok(qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Nested(
                qdrant_client::qdrant::NestedCondition {
                    key,
                    filter: inner_filter,
                },
            )),
        });
    }

    if obj.contains_key("must") || obj.contains_key("should") || obj.contains_key("must_not") {
        let filter = parse_json_filter(val)?;
        return Ok(qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Filter(
                filter,
            )),
        });
    }

    if let Some(key_val) = obj.get("key") {
        let key = key_val.as_str().unwrap_or_default().to_string();

        let mut match_val = None;
        let mut range_val = None;
        let geo_bounding_box = None;
        let mut geo_radius = None;
        let geo_polygon = None;
        let mut values_count = None;

        if let Some(m) = obj.get("match") {
            let match_value = if let Some(val_inner) = m.get("value") {
                let match_value = match val_inner {
                    serde_json::Value::Bool(b) => {
                        Some(qdrant_client::qdrant::r#match::MatchValue::Boolean(*b))
                    }
                    serde_json::Value::Number(n) => n
                        .as_i64()
                        .map(qdrant_client::qdrant::r#match::MatchValue::Integer),
                    serde_json::Value::String(s) => Some(
                        qdrant_client::qdrant::r#match::MatchValue::Keyword(s.clone()),
                    ),
                    _ => None,
                };
                Some(qdrant_client::qdrant::Match { match_value })
            } else if let Some(any_val) = m.get("any") {
                let any_arr = any_val.as_array().cloned().unwrap_or_default();
                let match_value = if any_arr.first().and_then(|v| v.as_i64()).is_some() {
                    let integers = any_arr.iter().filter_map(|v| v.as_i64()).collect();
                    Some(qdrant_client::qdrant::r#match::MatchValue::Integers(
                        qdrant_client::qdrant::RepeatedIntegers { integers },
                    ))
                } else {
                    let strings = any_arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    Some(qdrant_client::qdrant::r#match::MatchValue::Keywords(
                        qdrant_client::qdrant::RepeatedStrings { strings },
                    ))
                };
                Some(qdrant_client::qdrant::Match { match_value })
            } else if let Some(text_val) = m.get("text") {
                Some(qdrant_client::qdrant::Match {
                    match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Text(
                        text_val.as_str().unwrap_or_default().to_string(),
                    )),
                })
            } else {
                m.get("phrase")
                    .map(|phrase_val| qdrant_client::qdrant::Match {
                        match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Text(
                            phrase_val.as_str().unwrap_or_default().to_string(),
                        )),
                    })
            };
            match_val = match_value;
        }

        if let Some(r) = obj.get("range") {
            range_val = Some(qdrant_client::qdrant::Range {
                lt: r.get("lt").and_then(|v| v.as_f64()),
                gt: r.get("gt").and_then(|v| v.as_f64()),
                gte: r.get("gte").and_then(|v| v.as_f64()),
                lte: r.get("lte").and_then(|v| v.as_f64()),
            });
        }

        if let Some(vc) = obj.get("values_count") {
            values_count = Some(qdrant_client::qdrant::ValuesCount {
                lt: vc.get("lt").and_then(|v| v.as_u64()),
                gt: vc.get("gt").and_then(|v| v.as_u64()),
                gte: vc.get("gte").and_then(|v| v.as_u64()),
                lte: vc.get("lte").and_then(|v| v.as_u64()),
            });
        }

        if let Some(gr) = obj.get("geo_radius") {
            let center = gr.get("center").map(|p| qdrant_client::qdrant::GeoPoint {
                lat: p.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0),
                lon: p.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0),
            });
            let radius = gr.get("radius").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            geo_radius = Some(qdrant_client::qdrant::GeoRadius { center, radius });
        }

        return Ok(qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                qdrant_client::qdrant::FieldCondition {
                    key,
                    r#match: match_val,
                    range: range_val,
                    geo_bounding_box,
                    geo_radius,
                    geo_polygon,
                    values_count,
                    datetime_range: None,
                    is_empty: None,
                    is_null: None,
                },
            )),
        });
    }

    Err(QqlError::runtime(format!(
        "unknown condition JSON structure: {:?}",
        val
    )))
}

fn to_grpc_filter(
    filter: crate::backend::Filter,
) -> Result<qdrant_client::qdrant::Filter, QqlError> {
    parse_json_filter(filter.as_json())
}

fn to_grpc_vectors(val: &serde_json::Value) -> Result<qdrant_client::qdrant::Vectors, QqlError> {
    if val.is_null() {
        return Ok(qdrant_client::qdrant::Vectors::default());
    }

    if let Some(arr) = val.as_array() {
        let data: Vec<f32> = arr
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        return Ok(qdrant_client::qdrant::Vectors {
            vectors_options: Some(qdrant_client::qdrant::vectors::VectorsOptions::Vector(
                qdrant_client::qdrant::Vector {
                    vector: Some(qdrant_client::qdrant::vector::Vector::Dense(
                        qdrant_client::qdrant::DenseVector { data },
                    )),
                    ..Default::default()
                },
            )),
        });
    }

    if let Some(obj) = val.as_object() {
        let mut vectors_map = HashMap::new();
        for (name, v_val) in obj {
            if let Some(sparse_obj) = v_val.as_object() {
                if sparse_obj.contains_key("indices") && sparse_obj.contains_key("values") {
                    let indices: Vec<u32> = sparse_obj
                        .get("indices")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_u64().map(|n| n as u32))
                        .collect();
                    let values: Vec<f32> = sparse_obj
                        .get("values")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();
                    vectors_map.insert(
                        name.clone(),
                        qdrant_client::qdrant::Vector {
                            vector: Some(qdrant_client::qdrant::vector::Vector::Sparse(
                                qdrant_client::qdrant::SparseVector { indices, values },
                            )),
                            ..Default::default()
                        },
                    );
                    continue;
                }
            }
            if let Some(arr) = v_val.as_array() {
                let data: Vec<f32> = arr
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                vectors_map.insert(
                    name.clone(),
                    qdrant_client::qdrant::Vector {
                        vector: Some(qdrant_client::qdrant::vector::Vector::Dense(
                            qdrant_client::qdrant::DenseVector { data },
                        )),
                        ..Default::default()
                    },
                );
            }
        }
        return Ok(qdrant_client::qdrant::Vectors {
            vectors_options: Some(qdrant_client::qdrant::vectors::VectorsOptions::Vectors(
                qdrant_client::qdrant::NamedVectors {
                    vectors: vectors_map,
                },
            )),
        });
    }

    Err(QqlError::runtime("invalid vector format"))
}

fn from_grpc_vector_output(vec: qdrant_client::qdrant::VectorOutput) -> serde_json::Value {
    if let Some(v) = vec.vector {
        match v {
            qdrant_client::qdrant::vector_output::Vector::Dense(dense) => {
                serde_json::json!(dense.data)
            }
            qdrant_client::qdrant::vector_output::Vector::Sparse(sparse) => {
                serde_json::json!({
                    "indices": sparse.indices,
                    "values": sparse.values,
                })
            }
            qdrant_client::qdrant::vector_output::Vector::MultiDense(multi) => {
                let list: Vec<serde_json::Value> = multi
                    .vectors
                    .into_iter()
                    .map(|d| serde_json::json!(d.data))
                    .collect();
                serde_json::Value::Array(list)
            }
        }
    } else {
        serde_json::Value::Null
    }
}

fn from_grpc_vectors_output(vectors: qdrant_client::qdrant::VectorsOutput) -> serde_json::Value {
    if let Some(opts) = vectors.vectors_options {
        match opts {
            qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(vec) => {
                from_grpc_vector_output(vec)
            }
            qdrant_client::qdrant::vectors_output::VectorsOptions::Vectors(named) => {
                let mut map = serde_json::Map::new();
                for (name, vec) in named.vectors {
                    map.insert(name, from_grpc_vector_output(vec));
                }
                serde_json::Value::Object(map)
            }
        }
    } else {
        serde_json::Value::Null
    }
}

fn from_grpc_scored_point(p: qdrant_client::qdrant::ScoredPoint) -> Result<ScoredPoint, QqlError> {
    let id =
        p.id.ok_or_else(|| QqlError::runtime("gRPC scored point has no ID"))?;
    let id = from_grpc_point_id(id)?;

    let payload =
        if p.payload.is_empty() {
            None
        } else {
            let payload_val = serde_json::to_value(&p.payload).map_err(|e| {
                QqlError::runtime(format!("failed to serialize gRPC payload to JSON: {e}"))
            })?;
            Some(serde_json::from_value(payload_val).map_err(|e| {
                QqlError::runtime(format!("failed to deserialize payload to map: {e}"))
            })?)
        };

    let vector = p.vectors.map(from_grpc_vectors_output);

    Ok(ScoredPoint {
        id,
        score: p.score,
        payload,
        vector,
    })
}

fn from_grpc_retrieved_point(
    p: qdrant_client::qdrant::RetrievedPoint,
) -> Result<RetrievedPoint, QqlError> {
    let id =
        p.id.ok_or_else(|| QqlError::runtime("gRPC retrieved point has no ID"))?;
    let id = from_grpc_point_id(id)?;

    let payload = if p.payload.is_empty() {
        None
    } else {
        let payload_val = serde_json::to_value(&p.payload)
            .map_err(|e| QqlError::runtime(format!("failed to serialize gRPC payload: {e}")))?;
        Some(
            serde_json::from_value(payload_val)
                .map_err(|e| QqlError::runtime(format!("failed to deserialize payload: {e}")))?,
        )
    };

    let vector = p.vectors.map(from_grpc_vectors_output);

    Ok(RetrievedPoint {
        id,
        payload,
        vector,
    })
}

fn from_grpc_point_group(g: qdrant_client::qdrant::PointGroup) -> Result<PointGroup, QqlError> {
    let id = if let Some(gid) = g.id {
        match gid.kind {
            Some(qdrant_client::qdrant::group_id::Kind::UnsignedValue(val)) => {
                serde_json::json!(val)
            }
            Some(qdrant_client::qdrant::group_id::Kind::IntegerValue(val)) => {
                serde_json::json!(val)
            }
            Some(qdrant_client::qdrant::group_id::Kind::StringValue(val)) => serde_json::json!(val),
            None => serde_json::Value::Null,
        }
    } else {
        serde_json::Value::Null
    };

    let hits: Result<Vec<ScoredPoint>, QqlError> =
        g.hits.into_iter().map(from_grpc_scored_point).collect();

    Ok(PointGroup { id, hits: hits? })
}

fn to_grpc_vector_input(vi: crate::pipeline::VectorInput) -> qdrant_client::qdrant::VectorInput {
    let variant = match vi {
        crate::pipeline::VectorInput::Id(id) => Some(
            qdrant_client::qdrant::vector_input::Variant::Id(to_grpc_point_id(id)),
        ),
        crate::pipeline::VectorInput::Dense(vec) => {
            Some(qdrant_client::qdrant::vector_input::Variant::Dense(
                qdrant_client::qdrant::DenseVector { data: vec },
            ))
        }
        crate::pipeline::VectorInput::Document {
            text,
            model,
            options,
        } => {
            let grpc_options = options
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        qdrant_client::qdrant::Value {
                            kind: Some(qdrant_client::qdrant::value::Kind::StringValue(v)),
                        },
                    )
                })
                .collect();
            Some(qdrant_client::qdrant::vector_input::Variant::Document(
                qdrant_client::qdrant::Document {
                    text,
                    model,
                    options: grpc_options,
                },
            ))
        }
    };
    qdrant_client::qdrant::VectorInput { variant }
}

fn to_grpc_search_params(p: crate::pipeline::SearchParams) -> qdrant_client::qdrant::SearchParams {
    qdrant_client::qdrant::SearchParams {
        hnsw_ef: p.hnsw_ef,
        exact: p.exact,
        quantization: p
            .quantization
            .map(|q| qdrant_client::qdrant::QuantizationSearchParams {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }),
        indexed_only: p.indexed_only,
        acorn: p.acorn.map(|a| qdrant_client::qdrant::AcornSearchParams {
            enable: Some(a.enable),
            ..Default::default()
        }),
    }
}

fn to_grpc_lookup_location(
    lf: crate::pipeline::LookupLocation,
) -> qdrant_client::qdrant::LookupLocation {
    qdrant_client::qdrant::LookupLocation {
        collection_name: lf.collection_name,
        vector_name: lf.vector_name,
        shard_key_selector: None,
    }
}

fn parse_json_expression(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::Expression, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("expression must be a JSON object"))?;

    let variant = if let Some(constant) = obj.get("constant") {
        let c = constant
            .as_f64()
            .ok_or_else(|| QqlError::runtime("constant must be a float"))? as f32;
        qdrant_client::qdrant::expression::Variant::Constant(c)
    } else if let Some(variable) = obj.get("variable") {
        let v = variable
            .as_str()
            .ok_or_else(|| QqlError::runtime("variable must be a string"))?
            .to_string();
        qdrant_client::qdrant::expression::Variant::Variable(v)
    } else if let Some(datetime) = obj.get("datetime") {
        let dt = datetime
            .as_str()
            .ok_or_else(|| QqlError::runtime("datetime must be a string"))?
            .to_string();
        qdrant_client::qdrant::expression::Variant::Datetime(dt)
    } else if let Some(datetime_key) = obj.get("datetime_key") {
        let dtk = datetime_key
            .as_str()
            .ok_or_else(|| QqlError::runtime("datetime_key must be a string"))?
            .to_string();
        qdrant_client::qdrant::expression::Variant::DatetimeKey(dtk)
    } else if let Some(sum) = obj.get("sum") {
        let arr = sum
            .as_array()
            .ok_or_else(|| QqlError::runtime("sum must be an array"))?;
        let expressions: Result<Vec<qdrant_client::qdrant::Expression>, QqlError> =
            arr.iter().map(parse_json_expression).collect();
        qdrant_client::qdrant::expression::Variant::Sum(qdrant_client::qdrant::SumExpression {
            sum: expressions?,
        })
    } else if let Some(mult) = obj.get("mult") {
        let arr = mult
            .as_array()
            .ok_or_else(|| QqlError::runtime("mult must be an array"))?;
        let expressions: Result<Vec<qdrant_client::qdrant::Expression>, QqlError> =
            arr.iter().map(parse_json_expression).collect();
        qdrant_client::qdrant::expression::Variant::Mult(qdrant_client::qdrant::MultExpression {
            mult: expressions?,
        })
    } else if let Some(neg) = obj.get("neg") {
        let inner = parse_json_expression(neg)?;
        qdrant_client::qdrant::expression::Variant::Neg(Box::new(inner))
    } else if let Some(abs) = obj.get("abs") {
        let inner = parse_json_expression(abs)?;
        qdrant_client::qdrant::expression::Variant::Abs(Box::new(inner))
    } else if let Some(sqrt) = obj.get("sqrt") {
        let inner = parse_json_expression(sqrt)?;
        qdrant_client::qdrant::expression::Variant::Sqrt(Box::new(inner))
    } else if let Some(log10) = obj.get("log10") {
        let inner = parse_json_expression(log10)?;
        qdrant_client::qdrant::expression::Variant::Log10(Box::new(inner))
    } else if let Some(ln) = obj.get("ln") {
        let inner = parse_json_expression(ln)?;
        qdrant_client::qdrant::expression::Variant::Ln(Box::new(inner))
    } else if let Some(exp) = obj.get("exp") {
        let inner = parse_json_expression(exp)?;
        qdrant_client::qdrant::expression::Variant::Exp(Box::new(inner))
    } else if let Some(pow) = obj.get("pow") {
        let pow_obj = pow
            .as_object()
            .ok_or_else(|| QqlError::runtime("pow must be an object"))?;
        let base = pow_obj
            .get("base")
            .ok_or_else(|| QqlError::runtime("pow must have base"))?;
        let exponent = pow_obj
            .get("exponent")
            .ok_or_else(|| QqlError::runtime("pow must have exponent"))?;
        qdrant_client::qdrant::expression::Variant::Pow(Box::new(
            qdrant_client::qdrant::PowExpression {
                base: Some(Box::new(parse_json_expression(base)?)),
                exponent: Some(Box::new(parse_json_expression(exponent)?)),
            },
        ))
    } else if let Some(geo_distance) = obj.get("geo_distance") {
        let gd_obj = geo_distance
            .as_object()
            .ok_or_else(|| QqlError::runtime("geo_distance must be an object"))?;
        let origin = gd_obj
            .get("origin")
            .ok_or_else(|| QqlError::runtime("geo_distance must have origin"))?;
        let to = gd_obj
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| QqlError::runtime("geo_distance must have to"))?
            .to_string();
        let lat = origin.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let lon = origin.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
        qdrant_client::qdrant::expression::Variant::GeoDistance(
            qdrant_client::qdrant::GeoDistance {
                origin: Some(qdrant_client::qdrant::GeoPoint { lat, lon }),
                to,
            },
        )
    } else if let Some(condition) = obj.get("condition") {
        let cond = parse_json_condition(condition)?;
        qdrant_client::qdrant::expression::Variant::Condition(cond)
    } else if let Some(div) = obj.get("div") {
        let div_obj = div
            .as_object()
            .ok_or_else(|| QqlError::runtime("div must be an object"))?;
        let left = div_obj
            .get("left")
            .ok_or_else(|| QqlError::runtime("div must have left"))?;
        let right = div_obj
            .get("right")
            .ok_or_else(|| QqlError::runtime("div must have right"))?;
        let by_zero_default = div_obj
            .get("by_zero_default")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32);
        qdrant_client::qdrant::expression::Variant::Div(Box::new(
            qdrant_client::qdrant::DivExpression {
                left: Some(Box::new(parse_json_expression(left)?)),
                right: Some(Box::new(parse_json_expression(right)?)),
                by_zero_default,
            },
        ))
    } else if let Some(exp_decay) = obj.get("exp_decay") {
        let decay = parse_decay_expression(exp_decay)?;
        qdrant_client::qdrant::expression::Variant::ExpDecay(Box::new(decay))
    } else if let Some(gauss_decay) = obj.get("gauss_decay") {
        let decay = parse_decay_expression(gauss_decay)?;
        qdrant_client::qdrant::expression::Variant::GaussDecay(Box::new(decay))
    } else if let Some(lin_decay) = obj.get("lin_decay") {
        let decay = parse_decay_expression(lin_decay)?;
        qdrant_client::qdrant::expression::Variant::LinDecay(Box::new(decay))
    } else {
        return Err(QqlError::runtime(format!(
            "unknown expression JSON structure: {:?}",
            val
        )));
    };

    Ok(qdrant_client::qdrant::Expression {
        variant: Some(variant),
    })
}

fn parse_decay_expression(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::DecayParamsExpression, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("decay must be an object"))?;
    let x = obj
        .get("x")
        .ok_or_else(|| QqlError::runtime("decay must have x"))?;
    let x_expr = parse_json_expression(x)?;
    let target_expr = obj.get("target").map(parse_json_expression).transpose()?;
    let scale = obj.get("scale").and_then(|v| v.as_f64()).map(|f| f as f32);
    let midpoint = obj
        .get("midpoint")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);

    Ok(qdrant_client::qdrant::DecayParamsExpression {
        x: Some(Box::new(x_expr)),
        target: target_expr.map(Box::new),
        scale,
        midpoint,
    })
}

fn to_grpc_query(
    variant: crate::pipeline::QueryVariant,
) -> Result<qdrant_client::qdrant::Query, QqlError> {
    let grpc_variant = match variant {
        crate::pipeline::QueryVariant::Nearest(vec) => {
            qdrant_client::qdrant::query::Variant::Nearest(qdrant_client::qdrant::VectorInput {
                variant: Some(qdrant_client::qdrant::vector_input::Variant::Dense(
                    qdrant_client::qdrant::DenseVector { data: vec },
                )),
            })
        }
        crate::pipeline::QueryVariant::Sparse(indices, values) => {
            qdrant_client::qdrant::query::Variant::Nearest(qdrant_client::qdrant::VectorInput {
                variant: Some(qdrant_client::qdrant::vector_input::Variant::Sparse(
                    qdrant_client::qdrant::SparseVector { indices, values },
                )),
            })
        }
        crate::pipeline::QueryVariant::Document {
            text,
            model,
            options,
        } => {
            let grpc_options = options
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        qdrant_client::qdrant::Value {
                            kind: Some(qdrant_client::qdrant::value::Kind::StringValue(v)),
                        },
                    )
                })
                .collect();
            qdrant_client::qdrant::query::Variant::Nearest(qdrant_client::qdrant::VectorInput {
                variant: Some(qdrant_client::qdrant::vector_input::Variant::Document(
                    qdrant_client::qdrant::Document {
                        text,
                        model,
                        options: grpc_options,
                    },
                )),
            })
        }
        crate::pipeline::QueryVariant::Recommend(input) => {
            let positive = input
                .positive
                .into_iter()
                .map(to_grpc_vector_input)
                .collect();
            let negative = input
                .negative
                .into_iter()
                .map(to_grpc_vector_input)
                .collect();
            let strategy = input.strategy.map(|s| match s {
                crate::pipeline::RecommendStrategyType::AverageVector => 0,
                crate::pipeline::RecommendStrategyType::BestScore => 1,
                crate::pipeline::RecommendStrategyType::SumScores => 2,
            });
            qdrant_client::qdrant::query::Variant::Recommend(
                qdrant_client::qdrant::RecommendInput {
                    positive,
                    negative,
                    strategy,
                },
            )
        }
        crate::pipeline::QueryVariant::Context(input) => {
            let pairs = input
                .pairs
                .into_iter()
                .map(|p| qdrant_client::qdrant::ContextInputPair {
                    positive: p.positive.map(to_grpc_vector_input),
                    negative: p.negative.map(to_grpc_vector_input),
                })
                .collect();
            qdrant_client::qdrant::query::Variant::Context(qdrant_client::qdrant::ContextInput {
                pairs,
            })
        }
        crate::pipeline::QueryVariant::Discover(input) => {
            let context_pairs = input
                .context
                .pairs
                .into_iter()
                .map(|p| qdrant_client::qdrant::ContextInputPair {
                    positive: p.positive.map(to_grpc_vector_input),
                    negative: p.negative.map(to_grpc_vector_input),
                })
                .collect();
            qdrant_client::qdrant::query::Variant::Discover(qdrant_client::qdrant::DiscoverInput {
                target: Some(to_grpc_vector_input(input.target)),
                context: Some(qdrant_client::qdrant::ContextInput {
                    pairs: context_pairs,
                }),
            })
        }
        crate::pipeline::QueryVariant::OrderBy(input) => {
            let direction = match input.direction {
                crate::pipeline::OrderByDirection::Asc => 0,
                crate::pipeline::OrderByDirection::Desc => 1,
            };
            qdrant_client::qdrant::query::Variant::OrderBy(qdrant_client::qdrant::OrderBy {
                key: input.key,
                direction: Some(direction),
                start_from: None,
            })
        }
        crate::pipeline::QueryVariant::Sample => qdrant_client::qdrant::query::Variant::Sample(0),
        crate::pipeline::QueryVariant::Fusion(fusion_type) => {
            let val = match fusion_type {
                crate::pipeline::FusionType::Rrf => 0,
                crate::pipeline::FusionType::Dbsf => 1,
            };
            qdrant_client::qdrant::query::Variant::Fusion(val)
        }
        crate::pipeline::QueryVariant::Rrf(config) => {
            qdrant_client::qdrant::query::Variant::Rrf(qdrant_client::qdrant::Rrf {
                k: config.k,
                weights: config.weights,
            })
        }
        crate::pipeline::QueryVariant::Formula {
            expression,
            defaults,
        } => {
            let expr = parse_json_expression(&expression)?;
            let mut grpc_defaults = HashMap::new();
            for (key, val) in defaults {
                let val_json = serde_json::json!(val);
                let grpc_val: qdrant_client::qdrant::Value = serde_json::from_value(val_json)
                    .map_err(|e| QqlError::runtime(format!("invalid default value: {e}")))?;
                grpc_defaults.insert(key, grpc_val);
            }
            qdrant_client::qdrant::query::Variant::Formula(qdrant_client::qdrant::Formula {
                expression: Some(expr),
                defaults: grpc_defaults,
            })
        }
        crate::pipeline::QueryVariant::RelevanceFeedback(input) => {
            let feedback = input
                .feedback
                .into_iter()
                .map(|item| qdrant_client::qdrant::FeedbackItem {
                    example: Some(to_grpc_vector_input(item.example)),
                    score: item.score,
                })
                .collect();
            let strategy = input
                .strategy
                .map(|s| qdrant_client::qdrant::FeedbackStrategy {
                    variant: Some(qdrant_client::qdrant::feedback_strategy::Variant::Naive(
                        qdrant_client::qdrant::NaiveFeedbackStrategy {
                            a: s.a,
                            b: s.b,
                            c: s.c,
                        },
                    )),
                });
            qdrant_client::qdrant::query::Variant::RelevanceFeedback(
                qdrant_client::qdrant::RelevanceFeedbackInput {
                    target: Some(to_grpc_vector_input(input.target)),
                    feedback,
                    strategy,
                },
            )
        }
        crate::pipeline::QueryVariant::MMR {
            input,
            diversity,
            candidates,
        } => {
            let nearest = to_grpc_query(*input)?;
            let vector_input = match nearest.variant {
                Some(qdrant_client::qdrant::query::Variant::Nearest(vi)) => Some(vi),
                _ => return Err(QqlError::runtime("MMR inner query must be a nearest query")),
            };
            qdrant_client::qdrant::query::Variant::NearestWithMmr(
                qdrant_client::qdrant::NearestInputWithMmr {
                    nearest: vector_input,
                    mmr: Some(qdrant_client::qdrant::Mmr {
                        diversity: Some(diversity),
                        candidates_limit: Some(candidates),
                    }),
                },
            )
        }
    };
    Ok(qdrant_client::qdrant::Query {
        variant: Some(grpc_variant),
    })
}

fn to_grpc_with_payload(
    wp: &crate::pipeline::WithPayload,
) -> qdrant_client::qdrant::WithPayloadSelector {
    let selector_options = if !wp.exclude.is_empty() {
        Some(
            qdrant_client::qdrant::with_payload_selector::SelectorOptions::Exclude(
                qdrant_client::qdrant::PayloadExcludeSelector {
                    fields: wp.exclude.clone(),
                },
            ),
        )
    } else if !wp.include.is_empty() {
        Some(
            qdrant_client::qdrant::with_payload_selector::SelectorOptions::Include(
                qdrant_client::qdrant::PayloadIncludeSelector {
                    fields: wp.include.clone(),
                },
            ),
        )
    } else {
        Some(
            qdrant_client::qdrant::with_payload_selector::SelectorOptions::Enable(
                wp.enable.unwrap_or(false),
            ),
        )
    };
    qdrant_client::qdrant::WithPayloadSelector { selector_options }
}

fn to_grpc_with_vectors(
    wv: &crate::pipeline::WithVector,
) -> qdrant_client::qdrant::WithVectorsSelector {
    let selector_options = if !wv.vectors.is_empty() {
        Some(
            qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Include(
                qdrant_client::qdrant::VectorsSelector {
                    names: wv.vectors.clone(),
                },
            ),
        )
    } else {
        Some(
            qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Enable(
                wv.enable.unwrap_or(false),
            ),
        )
    };
    qdrant_client::qdrant::WithVectorsSelector { selector_options }
}

fn to_grpc_prefetch(
    pq: crate::pipeline::PrefetchQuery,
) -> Result<qdrant_client::qdrant::PrefetchQuery, QqlError> {
    let prefetch: Result<Vec<qdrant_client::qdrant::PrefetchQuery>, QqlError> =
        pq.prefetches.into_iter().map(to_grpc_prefetch).collect();

    let query = pq.query.map(to_grpc_query).transpose()?;
    let params = pq.params.map(to_grpc_search_params);
    let filter = pq.filter.map(to_grpc_filter).transpose()?;
    let lookup_from = pq.lookup_from.map(to_grpc_lookup_location);

    Ok(qdrant_client::qdrant::PrefetchQuery {
        prefetch: prefetch?,
        query,
        using: pq.using,
        filter,
        params,
        score_threshold: pq.score_threshold,
        limit: pq.limit,
        lookup_from,
    })
}

fn to_grpc_query_points(
    req: QueryPointsRequest,
) -> Result<qdrant_client::qdrant::QueryPoints, QqlError> {
    let prefetch: Result<Vec<qdrant_client::qdrant::PrefetchQuery>, QqlError> =
        req.prefetches.into_iter().map(to_grpc_prefetch).collect();

    let query = req.query.map(to_grpc_query).transpose()?;
    let params = req.params.map(to_grpc_search_params);
    let filter = req.filter.map(to_grpc_filter).transpose()?;
    let lookup_from = req.lookup_from.map(to_grpc_lookup_location);
    let with_payload = req.with_payload.map(|wp| to_grpc_with_payload(&wp));
    let with_vectors = req.with_vector.map(|wv| to_grpc_with_vectors(&wv));

    Ok(qdrant_client::qdrant::QueryPoints {
        collection_name: req.collection_name,
        prefetch: prefetch?,
        query,
        using: req.using,
        filter,
        params,
        score_threshold: req.score_threshold,
        limit: Some(req.limit),
        offset: Some(req.offset),
        with_vectors,
        with_payload,
        lookup_from,
        ..Default::default()
    })
}

impl TryFrom<QueryPointsRequest> for qdrant_client::qdrant::QueryPoints {
    type Error = QqlError;

    fn try_from(req: QueryPointsRequest) -> Result<Self, Self::Error> {
        to_grpc_query_points(req)
    }
}

fn to_grpc_query_point_groups(
    req: QueryPointsGroupsRequest,
) -> Result<qdrant_client::qdrant::QueryPointGroups, QqlError> {
    let prefetch: Result<Vec<qdrant_client::qdrant::PrefetchQuery>, QqlError> =
        req.prefetches.into_iter().map(to_grpc_prefetch).collect();

    let query = req.query.map(to_grpc_query).transpose()?;
    let params = req.params.map(to_grpc_search_params);
    let filter = req.filter.map(to_grpc_filter).transpose()?;
    let lookup_from = req.lookup_from.map(to_grpc_lookup_location);
    let with_payload = req.with_payload.map(|wp| to_grpc_with_payload(&wp));
    let with_vectors = req.with_vector.map(|wv| to_grpc_with_vectors(&wv));
    let with_lookup = req.with_lookup.map(|wl| qdrant_client::qdrant::WithLookup {
        collection: wl.collection,
        with_payload: None,
        with_vectors: None,
    });

    Ok(qdrant_client::qdrant::QueryPointGroups {
        collection_name: req.collection_name,
        prefetch: prefetch?,
        query,
        using: req.using,
        filter,
        params,
        score_threshold: req.score_threshold,
        with_vectors,
        with_payload,
        lookup_from,
        limit: Some(req.limit),
        group_size: Some(req.group_size),
        group_by: req.group_by,
        with_lookup,
        ..Default::default()
    })
}

fn parse_json_vectors_config(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::VectorsConfig, QqlError> {
    if let Some(obj) = val.as_object() {
        if obj.contains_key("size") {
            let params = parse_json_vector_params(val)?;
            return Ok(qdrant_client::qdrant::VectorsConfig {
                config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                    params,
                )),
            });
        } else {
            let mut map = HashMap::new();
            for (key, inner_val) in obj {
                let params = parse_json_vector_params(inner_val)?;
                map.insert(key.clone(), params);
            }
            return Ok(qdrant_client::qdrant::VectorsConfig {
                config: Some(qdrant_client::qdrant::vectors_config::Config::ParamsMap(
                    qdrant_client::qdrant::VectorParamsMap { map },
                )),
            });
        }
    }
    Err(QqlError::runtime("invalid vectors_config JSON"))
}

fn parse_json_vector_params(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::VectorParams, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("vector params must be an object"))?;
    let size = obj
        .get("size")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| QqlError::runtime("size is required in vector params"))?;
    let distance_str = obj
        .get("distance")
        .and_then(|v| v.as_str())
        .unwrap_or("Cosine")
        .to_string();
    let distance = match distance_str.to_lowercase().as_str() {
        "cosine" => qdrant_client::qdrant::Distance::Cosine as i32,
        "dot" => qdrant_client::qdrant::Distance::Dot as i32,
        "euclid" | "euclidean" => qdrant_client::qdrant::Distance::Euclid as i32,
        "manhattan" => qdrant_client::qdrant::Distance::Manhattan as i32,
        _ => {
            return Err(QqlError::runtime(format!(
                "unknown distance: {distance_str}"
            )))
        }
    };
    let on_disk = obj.get("on_disk").and_then(|v| v.as_bool());

    Ok(qdrant_client::qdrant::VectorParams {
        size,
        distance,
        hnsw_config: None,
        quantization_config: None,
        on_disk,
        datatype: None,
        multivector_config: None,
    })
}

fn parse_json_sparse_vectors_config(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::SparseVectorConfig, QqlError> {
    let mut map = HashMap::new();
    if let Some(obj) = val.as_object() {
        for (key, _inner_val) in obj {
            let params = qdrant_client::qdrant::SparseVectorParams {
                index: None,
                modifier: None,
            };
            map.insert(key.clone(), params);
        }
    }
    Ok(qdrant_client::qdrant::SparseVectorConfig { map })
}

fn parse_json_hnsw_config_diff(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::HnswConfigDiff, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("hnsw_config must be an object"))?;
    Ok(qdrant_client::qdrant::HnswConfigDiff {
        m: obj.get("m").and_then(|v| v.as_u64()),
        ef_construct: obj.get("ef_construct").and_then(|v| v.as_u64()),
        full_scan_threshold: obj.get("full_scan_threshold").and_then(|v| v.as_u64()),
        max_indexing_threads: obj.get("max_indexing_threads").and_then(|v| v.as_u64()),
        on_disk: obj.get("on_disk").and_then(|v| v.as_bool()),
        payload_m: obj.get("payload_m").and_then(|v| v.as_u64()),
        inline_storage: None,
    })
}

fn parse_json_optimizers_config_diff(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::OptimizersConfigDiff, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("optimizers_config must be an object"))?;
    Ok(qdrant_client::qdrant::OptimizersConfigDiff {
        deleted_threshold: obj.get("deleted_threshold").and_then(|v| v.as_f64()),
        vacuum_min_vector_number: obj.get("vacuum_min_vector_number").and_then(|v| v.as_u64()),
        default_segment_number: obj.get("default_segment_number").and_then(|v| v.as_u64()),
        max_segment_size: obj.get("max_segment_size").and_then(|v| v.as_u64()),
        memmap_threshold: obj.get("memmap_threshold").and_then(|v| v.as_u64()),
        indexing_threshold: obj.get("indexing_threshold").and_then(|v| v.as_u64()),
        flush_interval_sec: obj.get("flush_interval_sec").and_then(|v| v.as_u64()),
        deprecated_max_optimization_threads: None,
        max_optimization_threads: obj.get("max_optimization_threads").and_then(|v| {
            if let Some(n) = v.as_u64() {
                Some(qdrant_client::qdrant::MaxOptimizationThreads {
                    variant: Some(
                        qdrant_client::qdrant::max_optimization_threads::Variant::Value(n),
                    ),
                })
            } else if let Some(s) = v.as_str() {
                if s.to_lowercase() == "auto" {
                    Some(qdrant_client::qdrant::MaxOptimizationThreads {
                        variant: Some(
                            qdrant_client::qdrant::max_optimization_threads::Variant::Setting(0),
                        ),
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }),
        prevent_unoptimized: None,
    })
}

fn parse_json_quantization_config(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::QuantizationConfig, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("quantization_config must be an object"))?;
    let quantization = if let Some(scalar) = obj.get("scalar") {
        let scalar_obj = scalar
            .as_object()
            .ok_or_else(|| QqlError::runtime("scalar quantization must be an object"))?;
        let r#type = match scalar_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("Int8")
        {
            "Int8" => qdrant_client::qdrant::QuantizationType::Int8 as i32,
            _ => qdrant_client::qdrant::QuantizationType::Int8 as i32,
        };
        let quantile = scalar_obj
            .get("quantile")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32);
        let always_ram = scalar_obj.get("always_ram").and_then(|v| v.as_bool());
        Some(
            qdrant_client::qdrant::quantization_config::Quantization::Scalar(
                qdrant_client::qdrant::ScalarQuantization {
                    r#type,
                    quantile,
                    always_ram,
                },
            ),
        )
    } else if let Some(product) = obj.get("product") {
        let product_obj = product
            .as_object()
            .ok_or_else(|| QqlError::runtime("product quantization must be an object"))?;
        let compression = match product_obj
            .get("compression")
            .and_then(|v| v.as_str())
            .unwrap_or("x8")
        {
            "x8" => qdrant_client::qdrant::CompressionRatio::X8 as i32,
            "x16" => qdrant_client::qdrant::CompressionRatio::X16 as i32,
            "x32" => qdrant_client::qdrant::CompressionRatio::X32 as i32,
            "x64" => qdrant_client::qdrant::CompressionRatio::X64 as i32,
            _ => qdrant_client::qdrant::CompressionRatio::X8 as i32,
        };
        let always_ram = product_obj.get("always_ram").and_then(|v| v.as_bool());
        Some(
            qdrant_client::qdrant::quantization_config::Quantization::Product(
                qdrant_client::qdrant::ProductQuantization {
                    compression,
                    always_ram,
                },
            ),
        )
    } else {
        None
    };
    Ok(qdrant_client::qdrant::QuantizationConfig { quantization })
}

fn parse_json_quantization_config_diff(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::QuantizationConfigDiff, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("quantization_config must be an object"))?;
    let quantization = if let Some(scalar) = obj.get("scalar") {
        let scalar_obj = scalar
            .as_object()
            .ok_or_else(|| QqlError::runtime("scalar quantization must be an object"))?;
        let r#type = match scalar_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("Int8")
        {
            "Int8" => qdrant_client::qdrant::QuantizationType::Int8 as i32,
            _ => qdrant_client::qdrant::QuantizationType::Int8 as i32,
        };
        let quantile = scalar_obj
            .get("quantile")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32);
        let always_ram = scalar_obj.get("always_ram").and_then(|v| v.as_bool());
        Some(
            qdrant_client::qdrant::quantization_config_diff::Quantization::Scalar(
                qdrant_client::qdrant::ScalarQuantization {
                    r#type,
                    quantile,
                    always_ram,
                },
            ),
        )
    } else if let Some(product) = obj.get("product") {
        let product_obj = product
            .as_object()
            .ok_or_else(|| QqlError::runtime("product quantization must be an object"))?;
        let compression = match product_obj
            .get("compression")
            .and_then(|v| v.as_str())
            .unwrap_or("x8")
        {
            "x8" => qdrant_client::qdrant::CompressionRatio::X8 as i32,
            "x16" => qdrant_client::qdrant::CompressionRatio::X16 as i32,
            "x32" => qdrant_client::qdrant::CompressionRatio::X32 as i32,
            "x64" => qdrant_client::qdrant::CompressionRatio::X64 as i32,
            _ => qdrant_client::qdrant::CompressionRatio::X8 as i32,
        };
        let always_ram = product_obj.get("always_ram").and_then(|v| v.as_bool());
        Some(
            qdrant_client::qdrant::quantization_config_diff::Quantization::Product(
                qdrant_client::qdrant::ProductQuantization {
                    compression,
                    always_ram,
                },
            ),
        )
    } else {
        None
    };
    Ok(qdrant_client::qdrant::QuantizationConfigDiff { quantization })
}

#[allow(dead_code)]
fn parse_json_collection_params_diff(
    val: &serde_json::Value,
) -> Result<qdrant_client::qdrant::CollectionParamsDiff, QqlError> {
    let obj = val
        .as_object()
        .ok_or_else(|| QqlError::runtime("params must be an object"))?;
    Ok(qdrant_client::qdrant::CollectionParamsDiff {
        replication_factor: obj
            .get("replication_factor")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        write_consistency_factor: obj
            .get("write_consistency_factor")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        read_fan_out_factor: obj
            .get("read_fan_out_factor")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        on_disk_payload: obj.get("on_disk_payload").and_then(|v| v.as_bool()),
        read_fan_out_delay_ms: None,
    })
}

#[async_trait]
impl QdrantCoreOps for GrpcQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let resp = self
            .client
            .list_collections()
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC list_collections failed: {e}")))?;
        Ok(resp.collections.into_iter().map(|c| c.name).collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        self.client
            .collection_exists(name)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC collection_exists failed: {e}")))
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let resp = self
            .client
            .collection_info(name)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC collection_info failed: {e}")))?;
        let info = resp.result.ok_or_else(|| {
            QqlError::runtime(format!("collection_info for '{}' returned no result", name))
        })?;

        let status = match info.status {
            1 => "green",
            2 => "yellow",
            3 => "red",
            _ => "unknown",
        }
        .to_string();

        let mut dense_vectors = Vec::new();
        let mut sparse_vectors = Vec::new();
        if let Some(ref config) = info.config {
            if let Some(ref params) = config.params {
                if let Some(ref vc) = params.vectors_config {
                    if let Some(ref cfg) = vc.config {
                        match cfg {
                            qdrant_client::qdrant::vectors_config::Config::Params(_) => {
                                dense_vectors.push(String::new());
                            }
                            qdrant_client::qdrant::vectors_config::Config::ParamsMap(ref map) => {
                                dense_vectors.extend(map.map.keys().cloned());
                            }
                        }
                    }
                }
                if let Some(ref svc) = params.sparse_vectors_config {
                    sparse_vectors.extend(svc.map.keys().cloned());
                }
            }
        }

        let has_sparse = !sparse_vectors.is_empty();

        let quantization_kind = if let Some(ref config) = info.config {
            if let Some(ref qc) = config.quantization_config {
                if let Some(ref q) = qc.quantization {
                    match q {
                        qdrant_client::qdrant::quantization_config::Quantization::Scalar(_) => {
                            Some("scalar")
                        }
                        qdrant_client::qdrant::quantization_config::Quantization::Product(_) => {
                            Some("product")
                        }
                        qdrant_client::qdrant::quantization_config::Quantization::Binary(_) => {
                            Some("binary")
                        }
                        _ => Some("scalar"),
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut payload_schema = serde_json::Map::new();
        for (field, idx_info) in &info.payload_schema {
            let schema_type =
                qdrant_client::qdrant::PayloadSchemaType::try_from(idx_info.data_type).ok();
            let data_type = match schema_type {
                Some(qdrant_client::qdrant::PayloadSchemaType::Text) => "text",
                Some(qdrant_client::qdrant::PayloadSchemaType::Integer) => "integer",
                Some(qdrant_client::qdrant::PayloadSchemaType::Float) => "float",
                Some(qdrant_client::qdrant::PayloadSchemaType::Geo) => "geo",
                Some(qdrant_client::qdrant::PayloadSchemaType::Bool) => "bool",
                Some(qdrant_client::qdrant::PayloadSchemaType::Datetime) => "datetime",
                Some(qdrant_client::qdrant::PayloadSchemaType::Uuid) => "uuid",
                _ => {
                    if let Some(ref p) = idx_info.params {
                        if let Some(ref ip) = p.index_params {
                            let dbg = format!("{:?}", ip);
                            if dbg.contains("TextIndexParams") {
                                "text"
                            } else {
                                "keyword"
                            }
                        } else {
                            "keyword"
                        }
                    } else {
                        "keyword"
                    }
                }
            };
            let mut entry = serde_json::Map::new();
            entry.insert(
                "type".to_string(),
                serde_json::Value::String(data_type.to_string()),
            );
            payload_schema.insert(field.clone(), serde_json::Value::Object(entry));
        }

        let raw_json = serde_json::json!({
            "status": status,
            "points_count": info.points_count.unwrap_or(0),
            "segments_count": info.segments_count as u64,
            "indexed_vectors_count": info.indexed_vectors_count.unwrap_or(0),
            "config": {
                "params": {
                    "sparse_vectors": if has_sparse { serde_json::json!({"sparse": {}}) } else { serde_json::json!({}) },
                },
                "quantization_config": quantization_kind.map(|kind| serde_json::json!({ kind: {} })),
            },
            "payload_schema": payload_schema,
        });

        Ok(CollectionInfo {
            status,
            points_count: info.points_count.unwrap_or(0),
            segments_count: info.segments_count as u64,
            schema: crate::backend::CollectionSchema {
                dense_vectors,
                sparse_vectors,
            },
            raw_json: Some(raw_json),
        })
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        let vectors_config = req
            .vectors_config
            .map(|v| parse_json_vectors_config(&v))
            .transpose()?;

        let sparse_vectors_config = req
            .sparse_vectors_config
            .map(|v| parse_json_sparse_vectors_config(&v))
            .transpose()?;

        let hnsw_config = req
            .hnsw_config
            .map(|v| parse_json_hnsw_config_diff(&v))
            .transpose()?;

        let optimizers_config = req
            .optimizers_config
            .map(|v| parse_json_optimizers_config_diff(&v))
            .transpose()?;

        let quantization_config = req
            .quantization_config
            .map(|v| parse_json_quantization_config(&v))
            .transpose()?;

        let mut shard_number = None;
        let mut on_disk_payload = None;
        let mut replication_factor = None;
        let mut write_consistency_factor = None;

        if let Some(p) = req.params {
            if let Some(obj) = p.as_object() {
                shard_number = obj
                    .get("shard_number")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                on_disk_payload = obj.get("on_disk_payload").and_then(|v| v.as_bool());
                replication_factor = obj
                    .get("replication_factor")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                write_consistency_factor = obj
                    .get("write_consistency_factor")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
            }
        }

        let grpc_req = qdrant_client::qdrant::CreateCollection {
            collection_name: req.collection_name,
            vectors_config,
            sparse_vectors_config,
            hnsw_config,
            optimizers_config,
            quantization_config,
            shard_number,
            on_disk_payload,
            replication_factor,
            write_consistency_factor,
            sharding_method: None,
            ..Default::default()
        };

        self.client
            .create_collection(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC create_collection failed: {e}")))?;
        Ok(())
    }

    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError> {
        let points: Result<Vec<qdrant_client::qdrant::PointStruct>, QqlError> = req
            .points
            .into_iter()
            .map(|point| {
                let id = to_grpc_point_id(point.id);
                let vectors = Some(to_grpc_vectors(&point.vector)?);
                let payload: qdrant_client::Payload = serde_json::from_value(
                    serde_json::Value::Object(point.payload.into_iter().collect()),
                )
                .map_err(|e| QqlError::runtime(format!("failed to parse payload: {e}")))?;

                Ok(qdrant_client::qdrant::PointStruct {
                    id: Some(id),
                    vectors,
                    payload: payload.into(),
                })
            })
            .collect();

        let grpc_req = qdrant_client::qdrant::UpsertPoints {
            collection_name: req.collection_name,
            wait: Some(true),
            points: points?,
            ..Default::default()
        };

        self.client
            .upsert_points(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC upsert failed: {e}")))?;
        Ok(())
    }

    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError> {
        let grpc_req = qdrant_client::qdrant::QueryPoints::try_from(req)?;
        let resp = self
            .client
            .query(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC query failed: {e}")))?;

        let scored_points: Result<Vec<ScoredPoint>, QqlError> = resp
            .result
            .into_iter()
            .map(from_grpc_scored_point)
            .collect();
        scored_points
    }

    async fn query_groups(
        &self,
        req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError> {
        let grpc_req = to_grpc_query_point_groups(req)?;
        let resp = self
            .client
            .query_groups(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC query_groups failed: {e}")))?;

        let groups: Result<Vec<PointGroup>, QqlError> = resp
            .result
            .ok_or_else(|| QqlError::runtime("query_groups returned no result"))?
            .groups
            .into_iter()
            .map(from_grpc_point_group)
            .collect();
        groups
    }

    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError> {
        let points_selector = if let Some(id) = req.point_id {
            Some(qdrant_client::qdrant::PointsSelector {
                points_selector_one_of: Some(
                    qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                        qdrant_client::qdrant::PointsIdsList {
                            ids: vec![to_grpc_point_id(id)],
                        },
                    ),
                ),
            })
        } else if let Some(filter) = req.filter {
            Some(qdrant_client::qdrant::PointsSelector {
                points_selector_one_of: Some(
                    qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Filter(
                        to_grpc_filter(filter)?,
                    ),
                ),
            })
        } else {
            None
        };

        let grpc_req = qdrant_client::qdrant::DeletePoints {
            collection_name: req.collection_name,
            wait: Some(true),
            points: points_selector,
            ..Default::default()
        };

        self.client
            .delete_points(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC delete failed: {e}")))?;
        Ok(())
    }

    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError> {
        let vector = match req.vector_name {
            Some(name) => serde_json::json!({ name: req.vector }),
            None => serde_json::json!(req.vector),
        };
        let vectors = to_grpc_vectors(&vector)?;
        let point_vectors = qdrant_client::qdrant::PointVectors {
            id: Some(to_grpc_point_id(req.point_id)),
            vectors: Some(vectors),
        };

        let grpc_req = qdrant_client::qdrant::UpdatePointVectors {
            collection_name: req.collection_name,
            wait: Some(true),
            points: vec![point_vectors],
            ..Default::default()
        };

        self.client
            .update_vectors(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC update_vectors failed: {e}")))?;
        Ok(())
    }

    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError> {
        let mut payload = HashMap::new();
        for (key, val) in req.payload {
            let val_json = serde_json::json!(val);
            let grpc_val: qdrant_client::qdrant::Value = serde_json::from_value(val_json)
                .map_err(|e| QqlError::runtime(format!("invalid payload value: {e}")))?;
            payload.insert(key, grpc_val);
        }

        let points_selector = if let Some(id) = req.point_id {
            Some(qdrant_client::qdrant::PointsSelector {
                points_selector_one_of: Some(
                    qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                        qdrant_client::qdrant::PointsIdsList {
                            ids: vec![to_grpc_point_id(id)],
                        },
                    ),
                ),
            })
        } else if let Some(filter) = req.filter {
            Some(qdrant_client::qdrant::PointsSelector {
                points_selector_one_of: Some(
                    qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Filter(
                        to_grpc_filter(filter)?,
                    ),
                ),
            })
        } else {
            None
        };

        let grpc_req = qdrant_client::qdrant::SetPayloadPoints {
            collection_name: req.collection_name,
            wait: Some(true),
            payload,
            points_selector,
            ..Default::default()
        };

        self.client
            .set_payload(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC set_payload failed: {e}")))?;
        Ok(())
    }

    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<PointId>), QqlError> {
        let filter = req.filter.map(to_grpc_filter).transpose()?;
        let offset = req.after.map(to_grpc_point_id);

        let grpc_req = qdrant_client::qdrant::ScrollPoints {
            collection_name: req.collection_name,
            filter,
            offset,
            limit: Some(req.limit as u32),
            with_payload: Some(qdrant_client::qdrant::WithPayloadSelector {
                selector_options: Some(
                    qdrant_client::qdrant::with_payload_selector::SelectorOptions::Enable(true),
                ),
            }),
            ..Default::default()
        };

        let resp = self
            .client
            .scroll(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC scroll failed: {e}")))?;

        let points: Result<Vec<RetrievedPoint>, QqlError> = resp
            .result
            .into_iter()
            .map(from_grpc_retrieved_point)
            .collect();

        let next_page_offset = resp.next_page_offset.map(from_grpc_point_id).transpose()?;

        Ok((points?, next_page_offset))
    }

    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        let id = crate::executor::helpers::to_point_id_static(&req.point_id)?;
        let grpc_req = qdrant_client::qdrant::GetPoints {
            collection_name: req.collection_name,
            ids: vec![to_grpc_point_id(id)],
            with_payload: Some(qdrant_client::qdrant::WithPayloadSelector {
                selector_options: Some(
                    qdrant_client::qdrant::with_payload_selector::SelectorOptions::Enable(true),
                ),
            }),
            ..Default::default()
        };

        let resp = self
            .client
            .get_points(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC get failed: {e}")))?;

        let points: Result<Vec<RetrievedPoint>, QqlError> = resp
            .result
            .into_iter()
            .map(from_grpc_retrieved_point)
            .collect();

        points
    }
}

#[async_trait]
impl QdrantAdminOps for GrpcQdrant {
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError> {
        let collection_name = req
            .get("collection_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| QqlError::runtime("collection_name is required"))?
            .to_string();

        let optimizers_config = req
            .get("optimizers_config")
            .map(parse_json_optimizers_config_diff)
            .transpose()?;

        let hnsw_config = req
            .get("hnsw_config")
            .map(parse_json_hnsw_config_diff)
            .transpose()?;

        let quantization_config = req
            .get("quantization_config")
            .map(parse_json_quantization_config_diff)
            .transpose()?;

        let params = req
            .get("params")
            .map(parse_json_collection_params_diff)
            .transpose()?;

        let grpc_req = qdrant_client::qdrant::UpdateCollection {
            collection_name,
            optimizers_config,
            hnsw_config,
            quantization_config,
            params,
            ..Default::default()
        };

        self.client
            .update_collection(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC update_collection failed: {e}")))?;
        Ok(())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        self.client
            .delete_collection(name)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC delete_collection failed: {e}")))?;
        Ok(())
    }

    async fn query_batch(
        &self,
        req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        if req.is_empty() {
            return Ok(Vec::new());
        }

        let collection_name = req[0].collection_name.clone();
        let mut query_points = Vec::new();
        for r in req {
            let grpc_req = to_grpc_query_points(r)?;
            query_points.push(grpc_req);
        }

        let grpc_req = qdrant_client::qdrant::QueryBatchPoints {
            collection_name,
            query_points,
            ..Default::default()
        };

        let resp = self
            .client
            .query_batch(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC query_batch failed: {e}")))?;

        let results: Result<Vec<Vec<ScoredPoint>>, QqlError> = resp
            .result
            .into_iter()
            .map(|batch_result| {
                batch_result
                    .result
                    .into_iter()
                    .map(from_grpc_scored_point)
                    .collect()
            })
            .collect();
        results
    }

    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError> {
        let options: HashMap<String, serde_json::Value> = req
            .options
            .into_iter()
            .map(|(key, value)| (key, crate::executor::helpers::value_to_json(&value)))
            .collect();

        let field_type = match req.field_type.to_lowercase().as_str() {
            "keyword" => qdrant_client::qdrant::FieldType::Keyword,
            "integer" => qdrant_client::qdrant::FieldType::Integer,
            "float" => qdrant_client::qdrant::FieldType::Float,
            "geo" => qdrant_client::qdrant::FieldType::Geo,
            "text" => qdrant_client::qdrant::FieldType::Text,
            "bool" => qdrant_client::qdrant::FieldType::Bool,
            "datetime" => qdrant_client::qdrant::FieldType::Datetime,
            "uuid" => qdrant_client::qdrant::FieldType::Uuid,
            _ => {
                return Err(QqlError::runtime(format!(
                    "unknown field type: {}",
                    req.field_type
                )))
            }
        } as i32;

        let field_index_params = if !options.is_empty() {
            let index_params = match req.field_type.to_lowercase().as_str() {
                "text" => {
                    let tokenizer = options
                        .get("tokenizer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("word")
                        .to_lowercase();
                    let tokenizer = match tokenizer.as_str() {
                        "prefix" => 1,
                        "whitespace" => 2,
                        "word" => 3,
                        "multilingual" => 4,
                        _ => 3,
                    };
                    let lowercase = options.get("lowercase").and_then(|v| v.as_bool());
                    let min_token_len = options.get("min_token_len").and_then(|v| v.as_u64());
                    let max_token_len = options.get("max_token_len").and_then(|v| v.as_u64());
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(
                        qdrant_client::qdrant::payload_index_params::IndexParams::TextIndexParams(
                            qdrant_client::qdrant::TextIndexParams {
                                tokenizer,
                                lowercase,
                                min_token_len,
                                max_token_len,
                                on_disk,
                                stopwords: None,
                                ..Default::default()
                            },
                        ),
                    )
                }
                "keyword" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(qdrant_client::qdrant::payload_index_params::IndexParams::KeywordIndexParams(
                        qdrant_client::qdrant::KeywordIndexParams {
                            on_disk,
                            enable_hnsw: None,
                            is_tenant: None,
                        }
                    ))
                }
                "integer" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    let lookup = options.get("lookup").and_then(|v| v.as_bool());
                    let range = options.get("range").and_then(|v| v.as_bool());
                    Some(qdrant_client::qdrant::payload_index_params::IndexParams::IntegerIndexParams(
                        qdrant_client::qdrant::IntegerIndexParams {
                            on_disk,
                            lookup,
                            range,
                            enable_hnsw: None,
                            is_principal: None,
                        }
                    ))
                }
                "float" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(
                        qdrant_client::qdrant::payload_index_params::IndexParams::FloatIndexParams(
                            qdrant_client::qdrant::FloatIndexParams {
                                on_disk,
                                enable_hnsw: None,
                                is_principal: None,
                            },
                        ),
                    )
                }
                "geo" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(
                        qdrant_client::qdrant::payload_index_params::IndexParams::GeoIndexParams(
                            qdrant_client::qdrant::GeoIndexParams {
                                on_disk,
                                enable_hnsw: None,
                            },
                        ),
                    )
                }
                "bool" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(
                        qdrant_client::qdrant::payload_index_params::IndexParams::BoolIndexParams(
                            qdrant_client::qdrant::BoolIndexParams {
                                on_disk,
                                enable_hnsw: None,
                            },
                        ),
                    )
                }
                "datetime" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(qdrant_client::qdrant::payload_index_params::IndexParams::DatetimeIndexParams(
                        qdrant_client::qdrant::DatetimeIndexParams {
                            on_disk,
                            enable_hnsw: None,
                            is_principal: None,
                        }
                    ))
                }
                "uuid" => {
                    let on_disk = options.get("on_disk").and_then(|v| v.as_bool());
                    Some(
                        qdrant_client::qdrant::payload_index_params::IndexParams::UuidIndexParams(
                            qdrant_client::qdrant::UuidIndexParams {
                                on_disk,
                                enable_hnsw: None,
                                is_tenant: None,
                            },
                        ),
                    )
                }
                _ => None,
            };
            Some(qdrant_client::qdrant::PayloadIndexParams { index_params })
        } else {
            None
        };

        let grpc_req = qdrant_client::qdrant::CreateFieldIndexCollection {
            collection_name: req.collection_name,
            field_name: req.field,
            field_type: Some(field_type),
            field_index_params,
            wait: Some(true),
            ..Default::default()
        };

        self.client
            .create_field_index(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC create_field_index failed: {e}")))?;
        Ok(())
    }

    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError> {
        let filter = req.filter.map(to_grpc_filter).transpose()?;
        let grpc_req = qdrant_client::qdrant::CountPoints {
            collection_name: req.collection_name,
            filter,
            exact: Some(true),
            ..Default::default()
        };

        let resp = self
            .client
            .count(grpc_req)
            .await
            .map_err(|e| QqlError::runtime(format!("gRPC count failed: {e}")))?;

        let count = resp
            .result
            .ok_or_else(|| QqlError::runtime("count returned no result"))?
            .count;

        Ok(count)
    }
}
