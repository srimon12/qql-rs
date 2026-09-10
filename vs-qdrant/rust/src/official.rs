//! Official Qdrant SDK (`qdrant-client` 1.19, gRPC/tonic) scenario
//! implementations — one method per benchmark scenario, symmetric with
//! `qql_side.rs`. This is the application code a Qdrant Rust user writes.

use std::collections::HashMap;

use anyhow::Result;
use qdrant_client::qdrant::{
    point_id::PointIdOptions, vectors, Condition, CreateCollectionBuilder,
    CreateFieldIndexCollectionBuilder, DeletePointsBuilder, Distance,
    FacetCountsBuilder, FieldType, Filter, Fusion, HnswConfigDiffBuilder,
    MultiVectorComparator, MultiVectorConfigBuilder, NamedVectors, PointId, PointStruct,
    PrefetchQueryBuilder, QueryPointsBuilder, Range, ScoredPoint, ScrollPointsBuilder, SearchParamsBuilder,
    SetPayloadPointsBuilder, SparseVector, SparseVectorParamsBuilder, SparseVectorsConfigBuilder,
    UpsertPointsBuilder, Value as PbValue, Vector, VectorParamsBuilder, Vectors,
    VectorsConfigBuilder,
};
use qdrant_client::Qdrant;

/// Re-export for the harness (protobuf sparse vector).
pub type PbSparseVector = SparseVector;

pub const BATCH_BERLIN: usize = 100;
pub const BATCH_LEGAL: usize = 32;

pub struct OfficialScenarios {
    client: Qdrant,
}

fn pb_value(v: &serde_json::Value) -> PbValue {
    use qdrant_client::qdrant::value::Kind;
    PbValue {
        kind: Some(match v {
            serde_json::Value::Null => Kind::NullValue(0),
            serde_json::Value::Bool(b) => Kind::BoolValue(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Kind::IntegerValue(i)
                } else {
                    Kind::DoubleValue(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => Kind::StringValue(s.clone()),
            serde_json::Value::Array(_) => Kind::ListValue(qdrant_client::qdrant::ListValue {
                values: v.as_array().unwrap().iter().map(pb_value).collect(),
            }),
            serde_json::Value::Object(_) => Kind::StructValue(qdrant_client::qdrant::Struct {
                fields: v.as_object().unwrap()
                    .iter().map(|(k, val)| (k.clone(), pb_value(val))).collect(),
            }),
        }),
    }
}

fn json_payload(doc: &serde_json::Value) -> HashMap<String, PbValue> {
    doc.as_object().unwrap()
        .iter()
        .filter(|(k, _)| k.as_str() != "id")
        .map(|(k, v)| (k.clone(), pb_value(v)))
        .collect()
}

pub(crate) fn point_id_num(pid: &Option<PointId>) -> u64 {
    match pid.as_ref().and_then(|p| p.point_id_options.as_ref()) {
        Some(PointIdOptions::Num(n)) => *n,
        _ => 0,
    }
}

fn named_vectors(map: Vec<(&str, Vector)>) -> Vectors {
    Vectors {
        vectors_options: Some(vectors::VectorsOptions::Vectors(NamedVectors {
            vectors: map.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        })),
    }
}

fn sparse_of_idx(sv: &serde_json::Value) -> Vec<u32> {
    sv["indices"].as_array().unwrap()
        .iter().map(|v| v.as_u64().unwrap() as u32).collect()
}

fn f32s_val(sv: &serde_json::Value) -> Vec<f32> {
    sv["values"].as_array().unwrap()
        .iter().map(|v| v.as_f64().unwrap() as f32).collect()
}

impl OfficialScenarios {
    pub async fn new(url: &str) -> Result<Self> {
        Ok(Self { client: Qdrant::from_url(url).build()? })
    }

    // ------------------------------------------------------------ setup ----
    pub async fn drop_collection(&self, name: &str) -> Result<()> {
        let _ = self.client.delete_collection(name.to_string()).await;
        Ok(())
    }

    pub async fn create_berlin(&self, name: &str) -> Result<()> {
        self.client
            .create_collection(
                CreateCollectionBuilder::new(name)
                    .vectors_config({
                        VectorsConfigBuilder::default()
                            .add_named_vector_params(
                                "dense", VectorParamsBuilder::new(384, Distance::Cosine))
                            .to_owned()
                    })
                    .sparse_vectors_config({
                        SparseVectorsConfigBuilder::default()
                            .add_named_vector_params(
                                "bm25",
                                SparseVectorParamsBuilder::default().modifier(
                                    qdrant_client::qdrant::Modifier::Idf))
                            .to_owned()
                    }),
            )
            .await?;
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                name, "district", FieldType::Keyword))
            .await?;
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                name, "price", FieldType::Float))
            .await?;
        Ok(())
    }

    pub async fn create_legal(&self, name: &str) -> Result<()> {
        self.client
            .create_collection(
                CreateCollectionBuilder::new(name)
                    .vectors_config({
                        VectorsConfigBuilder::default()
                            .add_named_vector_params(
                                "dense", VectorParamsBuilder::new(384, Distance::Cosine))
                            .add_named_vector_params("colbert", {
                                VectorParamsBuilder::new(128, Distance::Cosine)
                                    .hnsw_config(HnswConfigDiffBuilder::default().m(0))
                                    .multivector_config(MultiVectorConfigBuilder::new(
                                        MultiVectorComparator::MaxSim))
                            })
                            .to_owned()
                    })
                    .sparse_vectors_config({
                        SparseVectorsConfigBuilder::default()
                            .add_named_vector_params(
                                "bm25",
                                SparseVectorParamsBuilder::default().modifier(
                                    qdrant_client::qdrant::Modifier::Idf))
                            .to_owned()
                    }),
            )
            .await?;
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                name, "court", FieldType::Keyword))
            .await?;
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                name, "year", FieldType::Integer))
            .await?;
        Ok(())
    }

    // ----------------------------------------------------------- ingest ----
    pub async fn ingest_berlin(
        &self, name: &str, docs: &[serde_json::Value],
        dense: &[f32], sparse: &[serde_json::Value],
    ) -> Result<()> {
        for start in (0..docs.len()).step_by(BATCH_BERLIN) {
            let end = (start + BATCH_BERLIN).min(docs.len());
            let points: Vec<PointStruct> = (start..end)
                .map(|i| PointStruct {
                    id: Some(PointId::from(docs[i]["id"].as_u64().unwrap())),
                    payload: json_payload(&docs[i]),
                    vectors: Some(named_vectors(vec![
                        ("dense", Vector::new_dense(dense[i * 384..(i + 1) * 384].to_vec())),
                        ("bm25", Vector::new_sparse(sparse_of_idx(&sparse[i]), f32s_val(&sparse[i]))),
                    ])),
                })
                .collect();
            self.client
                .upsert_points(UpsertPointsBuilder::new(name, points).wait(false))
                .await?;
        }
        Ok(())
    }

    pub async fn ingest_legal(
        &self, name: &str, docs: &[serde_json::Value],
        dense: &[f32], sparse: &[serde_json::Value],
        colbert_flat: &[f32], colbert_lens: &[usize],
    ) -> Result<()> {
        let mut offsets = Vec::with_capacity(colbert_lens.len() + 1);
        offsets.push(0usize);
        for l in colbert_lens {
            offsets.push(offsets.last().unwrap() + l * 128);
        }
        for start in (0..docs.len()).step_by(BATCH_LEGAL) {
            let end = (start + BATCH_LEGAL).min(docs.len());
            let points: Vec<PointStruct> = (start..end)
                .map(|i| {
                    let (o0, o1) = (offsets[i], offsets[i + 1]);
                    let mv: Vec<Vec<f32>> = colbert_flat[o0..o1]
                        .chunks(128).map(|c| c.to_vec()).collect();
                    PointStruct {
                        id: Some(PointId::from(docs[i]["id"].as_u64().unwrap())),
                        payload: json_payload(&docs[i]),
                        vectors: Some(named_vectors(vec![
                            ("dense", Vector::new_dense(
                                dense[i * 384..(i + 1) * 384].to_vec())),
                            ("colbert", Vector::new_multi(mv)),
                            ("bm25", Vector::new_sparse(sparse_of_idx(&sparse[i]), f32s_val(&sparse[i]))),
                        ])),
                    }
                })
                .collect();
            self.client
                .upsert_points(UpsertPointsBuilder::new(name, points).wait(false))
                .await?;
        }
        Ok(())
    }

    // ------------------------------------------------------------- reads ----
    pub async fn query_dense(&self, name: &str, qvec: &[f32]) -> Result<Vec<ScoredPoint>> {
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(name)
                    .query(qvec.to_vec())
                    .using("dense")
                    .limit(10)
                    .with_payload(true),
            )
            .await?;
        Ok(res.result)
    }

    pub async fn query_dense_filtered(
        &self, name: &str, qvec: &[f32],
    ) -> Result<Vec<ScoredPoint>> {
        let filter = Filter::must([
            Condition::range("price", Range { lt: Some(150.0), ..Default::default() }),
            Condition::range("guests", Range { gte: Some(2.0), ..Default::default() }),
        ]);
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(name)
                    .query(qvec.to_vec())
                    .using("dense")
                    .filter(filter)
                    .limit(10)
                    .with_payload(true),
            )
            .await?;
        Ok(res.result)
    }

    pub async fn query_sparse(
        &self, name: &str, sv: &SparseVector,
    ) -> Result<Vec<ScoredPoint>> {
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(name)
                    .query(sv.indices.iter().copied()
                        .zip(sv.values.iter().copied()).collect::<Vec<_>>())
                    .using("bm25")
                    .limit(10)
                    .with_payload(true),
            )
            .await?;
        Ok(res.result)
    }

    pub async fn query_hybrid(
        &self, name: &str, qvec: &[f32], sv: &SparseVector,
    ) -> Result<Vec<ScoredPoint>> {
        // hnsw_ef=128 on the dense leg: fused rankings must be deterministic
        // across the two independently built collections.
        let dense_q = PrefetchQueryBuilder::default()
            .query(qvec.to_vec())
            .using("dense")
            .limit(50u64)
            .params(SearchParamsBuilder::default().hnsw_ef(128));
        let sparse_q = PrefetchQueryBuilder::default()
            .query(sv.indices.iter().copied().zip(sv.values.iter().copied()).collect::<Vec<_>>())
            .using("bm25")
            .limit(50u64);
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(name)
                    .prefetch(vec![dense_q.build(), sparse_q.build()])
                    .query(Fusion::Rrf)
                    .limit(10)
                    .with_payload(true),
            )
            .await?;
        Ok(res.result)
    }

    pub async fn query_colbert(
        &self, name: &str, mvec: Vec<Vec<f32>>,
    ) -> Result<Vec<ScoredPoint>> {
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(name)
                    .query(mvec)
                    .using("colbert")
                    .limit(10)
                    .with_payload(true),
            )
            .await?;
        Ok(res.result)
    }

    pub async fn scroll_pages(&self, name: &str, pages: usize, batch: u32) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        let mut offset: Option<PointId> = None;
        for _ in 0..pages {
            let mut builder = ScrollPointsBuilder::new(name.to_string())
                .limit(batch)
                .with_payload(true);
            if let Some(o) = offset.take() {
                builder = builder.offset(o);
            }
            let res = self.client.scroll(builder).await?;
            ids.extend(res.result.iter().map(|p| point_id_num(&p.id)));
            match res.next_page_offset {
                Some(next) => offset = Some(next),
                None => break,
            }
        }
        Ok(ids)
    }

    pub async fn count_berlin(&self, name: &str) -> Result<u64> {
        let filter = Filter::must([Condition::range(
            "price", Range { lt: Some(150.0), ..Default::default() })]);
        Ok(self
            .client
            .count(qdrant_client::qdrant::CountPointsBuilder::new(name.to_string())
                .filter(filter)
                .exact(true))
            .await?
            .result.unwrap().count)
    }

    pub async fn count_legal(&self, name: &str) -> Result<u64> {
        let filter = Filter::must([Condition::range(
            "year", Range { gte: Some(2010.0), ..Default::default() })]);
        Ok(self
            .client
            .count(qdrant_client::qdrant::CountPointsBuilder::new(name.to_string())
                .filter(filter)
                .exact(true))
            .await?
            .result.unwrap().count)
    }

    pub async fn facet_district(&self, name: &str) -> Result<Vec<(String, u64)>> {
        let res = self
            .client
            .facet(FacetCountsBuilder::new(name.to_string(), "district".to_string())
                .limit(20)
                .exact(true))
            .await?;
        Ok(res
            .hits
            .into_iter()
            .map(|h| {
                let value = h.value.and_then(|v| v.variant).map(|variant| match variant {
                    qdrant_client::qdrant::facet_value::Variant::StringValue(s) => s,
                    qdrant_client::qdrant::facet_value::Variant::IntegerValue(i) => i.to_string(),
                    qdrant_client::qdrant::facet_value::Variant::BoolValue(b) => b.to_string(),
                }).unwrap_or_default();
                (value, h.count)
            })
            .collect())
    }

    // ----------------------------------------------------------- writes ----
    pub async fn update_payload(&self, name: &str) -> Result<()> {
        let filter = Filter::must([Condition::matches("district", "Mitte".to_string())]);
        self.client
            .set_payload(
                SetPayloadPointsBuilder::new(
                    name.to_string(),
                    HashMap::from([("rating".to_string(), pb_value(&serde_json::json!(4.5)))]),
                )
                .points_selector(filter)
                .wait(true),
            )
            .await?;
        Ok(())
    }

    pub async fn delete_by_filter(&self, name: &str) -> Result<()> {
        let filter = Filter::must([Condition::range(
            "price", Range { gt: Some(250.0), ..Default::default() })]);
        self.client
            .delete_points(
                DeletePointsBuilder::new(name.to_string())
                    .points(filter)
                    .wait(true),
            )
            .await?;
        Ok(())
    }

    pub async fn prepared_rerun(
        &self, name: &str, qvecs: &[Vec<f32>],
    ) -> Result<Vec<ScoredPoint>> {
        // No prepared statements in the official SDK: repeat the call.
        let mut hits = Vec::new();
        for v in qvecs {
            hits = self.query_dense(name, v).await?;
        }
        Ok(hits)
    }
}
