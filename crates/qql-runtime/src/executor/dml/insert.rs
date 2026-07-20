use serde_json;
use std::collections::HashMap;

#[cfg(feature = "rest")]
use crate::embedder::HttpEmbedder;
use crate::executor::{
    CreateCollectionReq, ExecResponse, Executor, PointStruct, UpsertPointsReq, VectorTopology,
};
use crate::sparse;
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

use crate::executor::helpers::value_to_json;

use super::helpers::*;

impl Executor {
    pub(crate) async fn do_insert(&self, stmt: ast::InsertStmt) -> Result<ExecResponse, QqlError> {
        if stmt.values_list.is_empty() {
            return Err(QqlError::runtime("INSERT VALUES list is empty"));
        }

        let _created = self
            .ensure_collection_for_insert(
                &stmt.collection,
                stmt.model.as_deref(),
                stmt.hybrid,
                stmt.dense_vector.as_deref(),
                stmt.sparse_vector.as_deref(),
            )
            .await?;

        let has_embed = !stmt.embed_directives.is_empty();
        let has_provided_vectors = has_vector_keys(&stmt.values_list);

        // Detect hybrid mode from EMBED directives or collection topology
        let has_sparse = stmt.hybrid
            || stmt.sparse_model.is_some()
            || stmt
                .embed_directives
                .iter()
                .any(|d| d.sparse_model.is_some());

        // 1. Extract point IDs and payloads from all rows
        let mut point_ids = Vec::with_capacity(stmt.values_list.len());
        let mut payloads = Vec::with_capacity(stmt.values_list.len());

        for row in &stmt.values_list {
            let id = extract_point_id(row)?;
            point_ids.push(id);

            let mut payload = row
                .iter()
                .filter(|(k, _)| *k != "id" && !is_vector_key(k))
                .map(|(k, v)| (k.to_string(), value_to_json(v)))
                .collect::<HashMap<_, _>>();
            // Strip vector keys from payload
            payload.retain(|k, _| !is_vector_key(k));
            payloads.push(payload);
        }

        // 2. Build vectors batch
        let vectors_batch = if has_embed {
            self.build_embed_vectors_batch(&stmt.values_list, &stmt.embed_directives)
                .await?
        } else if has_provided_vectors {
            extract_provided_vectors(&stmt.values_list)?
        } else {
            self.build_auto_embed_vectors_batch(
                &stmt.values_list,
                stmt.model.as_deref(),
                has_sparse,
            )
            .await?
        };

        // 3. Build and upsert points
        let points = point_ids
            .into_iter()
            .zip(payloads)
            .zip(vectors_batch)
            .map(|((id, payload), vectors)| PointStruct {
                id,
                vector: vectors.unwrap_or_else(|| serde_json::json!({})),
                payload,
            })
            .collect();

        let req = UpsertPointsReq {
            collection_name: stmt.collection.to_string(),
            points,
        };

        self.client.upsert(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "insert".to_string(),
            message: format!("Inserted {} point(s)", stmt.values_list.len()),
            data: Some(serde_json::json!({"count": stmt.values_list.len()})),
        })
    }

    async fn build_embed_vectors_batch(
        &self,
        values_list: &[Vec<(String, Value)>],
        directives: &[ast::EmbedDirective],
    ) -> Result<Vec<Option<serde_json::Value>>, QqlError> {
        // Validate no duplicate target vectors
        let mut seen = std::collections::HashSet::new();
        for dir in directives {
            if !seen.insert(dir.target_vector.clone()) {
                return Err(QqlError::runtime(format!(
                    "EMBED duplicate target vector '{}'",
                    dir.target_vector
                )));
            }
        }

        let mut batch = Vec::with_capacity(values_list.len());
        for (row_idx, row) in values_list.iter().enumerate() {
            let mut vectors = serde_json::Map::new();
            for dir in directives {
                let source_val = row
                    .iter()
                    .find(|(k, _)| *k == dir.source_field)
                    .map(|(_, v)| v)
                    .ok_or_else(|| {
                        QqlError::runtime(format!(
                            "EMBED row {}: source field '{}' not found in VALUES",
                            row_idx, dir.source_field
                        ))
                    })?;

                let source_text = match source_val {
                    Value::Str(s) => s.to_string(),
                    _ => {
                        return Err(QqlError::runtime(format!(
                            "EMBED row {}: source field '{}' must be a string",
                            row_idx, dir.source_field
                        )));
                    }
                };

                let is_sparse = dir.sparse_model.is_some();
                let model = if is_sparse {
                    dir.sparse_model
                        .as_ref()
                        .filter(|m| !m.is_empty())
                        .cloned()
                        .unwrap_or_else(|| self.resolve_sparse_model(None))
                } else {
                    dir.model
                        .as_ref()
                        .filter(|m| !m.is_empty())
                        .cloned()
                        .unwrap_or_else(|| self.resolve_dense_model(None))
                };

                let vector = if self.uses_local_embeddings() {
                    if is_sparse {
                        let sv = if let Some(ref embedder) = self.embedder {
                            embedder.embed_sparse(&source_text).await?
                        } else {
                            sparse::build_query_default(&source_text)
                        };
                        serde_json::json!({
                            "indices": sv.indices,
                            "values": sv.values,
                        })
                    } else {
                        let embedder = self.embedder.as_ref().ok_or_else(|| {
                            QqlError::runtime("local embedding requested but no Embedder provided")
                        })?;
                        let dv = embedder.embed_dense(&source_text, &model).await?;
                        serde_json::Value::Array(
                            dv.into_iter().map(|f| serde_json::json!(f)).collect(),
                        )
                    }
                } else {
                    serde_json::json!({
                        "text": source_text,
                        "model": model,
                        "options": self.cloud_model_options(),
                    })
                };

                vectors.insert(dir.target_vector.to_string(), vector);
            }
            batch.push(Some(serde_json::Value::Object(vectors)));
        }
        Ok(batch)
    }

    async fn build_auto_embed_vectors_batch(
        &self,
        values_list: &[Vec<(String, Value)>],
        model: Option<&str>,
        has_sparse: bool,
    ) -> Result<Vec<Option<serde_json::Value>>, QqlError> {
        // Collect texts that need dense embedding
        let mut texts: Vec<String> = Vec::new();
        let mut text_indices: Vec<usize> = Vec::new(); // maps text index → row index

        for (i, row) in values_list.iter().enumerate() {
            let text = row
                .iter()
                .find(|(k, _)| *k == "text" || *k == "description" || *k == "content")
                .and_then(|(_, v)| match v {
                    Value::Str(s) => Some(s.to_string()),
                    _ => None,
                });

            match text {
                Some(t) if !t.is_empty() => {
                    text_indices.push(i);
                    texts.push(t);
                }
                _ => {}
            }
        }

        let dense_model = self.resolve_dense_model(model);

        // Build dense vectors
        let dense_vectors: Option<Vec<serde_json::Value>> = if !texts.is_empty() {
            if self.uses_local_embeddings() {
                if let Some(ref embedder) = self.embedder {
                    let dv_list = embedder.embed_dense_batch(&texts, &dense_model).await?;
                    Some(
                        dv_list
                            .into_iter()
                            .map(|dv| {
                                serde_json::Value::Array(
                                    dv.into_iter().map(|f| serde_json::json!(f)).collect(),
                                )
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            } else {
                let cloud_opts = self.cloud_model_options();
                Some(
                    texts
                        .iter()
                        .map(|text| {
                            serde_json::json!({
                                "text": text,
                                "model": dense_model,
                                "options": cloud_opts,
                            })
                        })
                        .collect(),
                )
            }
        } else {
            None
        };

        // Build sparse vectors if hybrid
        let sparse_vectors: Option<Vec<serde_json::Value>> = if has_sparse && !texts.is_empty() {
            if self.uses_local_embeddings() {
                let mut sv_list = Vec::with_capacity(texts.len());
                for text in &texts {
                    let sv = if let Some(ref embedder) = self.embedder {
                        embedder.embed_sparse(text).await?
                    } else {
                        crate::sparse::build_query_default(text)
                    };
                    sv_list.push(serde_json::json!({
                        "indices": sv.indices,
                        "values": sv.values,
                    }));
                }
                Some(sv_list)
            } else {
                let cloud_opts = self.cloud_model_options();
                let sparse_model = self.resolve_sparse_model(None);
                Some(
                    texts
                        .iter()
                        .map(|text| {
                            serde_json::json!({
                                "text": text,
                                "model": sparse_model,
                                "options": cloud_opts,
                            })
                        })
                        .collect(),
                )
            }
        } else {
            None
        };

        let dense_name = crate::executor::DENSE_VECTOR_NAME;

        let mut batch = Vec::with_capacity(values_list.len());
        for i in 0..values_list.len() {
            // Find the text index for this row
            let pos = text_indices.iter().position(|&idx| idx == i);
            let vec_val = match (pos, &dense_vectors, &sparse_vectors) {
                (Some(p), Some(dv), Some(sv)) if p < dv.len() && p < sv.len() => {
                    let mut map = serde_json::Map::new();
                    map.insert(dense_name.to_string(), dv[p].clone());
                    map.insert(
                        crate::executor::SPARSE_VECTOR_NAME.to_string(),
                        sv[p].clone(),
                    );
                    Some(serde_json::Value::Object(map))
                }
                (Some(p), Some(dv), None) if p < dv.len() => {
                    let mut map = serde_json::Map::new();
                    map.insert(dense_name.to_string(), dv[p].clone());
                    Some(serde_json::Value::Object(map))
                }
                _ => None,
            };
            batch.push(vec_val);
        }

        Ok(batch)
    }

    pub(crate) async fn ensure_collection_for_insert(
        &self,
        collection: &str,
        model: Option<&str>,
        requested_hybrid: bool,
        explicit_dense: Option<&str>,
        explicit_sparse: Option<&str>,
    ) -> Result<bool, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if exists {
            return Ok(false);
        }

        let dense_size = self.resolve_dense_vector_size(model).await?;
        let dense_name = explicit_dense.unwrap_or(crate::executor::DENSE_VECTOR_NAME);

        let mut create_req = CreateCollectionReq::new(collection.to_string());
        create_req.vectors_config = Some(serde_json::json!({
            dense_name: {
                "size": dense_size,
                "distance": "Cosine"
            }
        }));

        if requested_hybrid {
            let sparse_name = explicit_sparse.unwrap_or(crate::executor::SPARSE_VECTOR_NAME);
            create_req.sparse_vectors_config = Some(serde_json::json!({
                sparse_name: {
                    "modifier": "idf"
                }
            }));
        }

        self.client.create_collection(create_req).await?;

        Ok(true)
    }

    pub(crate) async fn resolve_vector_topology(
        &self,
        collection: &str,
    ) -> Result<VectorTopology, QqlError> {
        let info = self.client.get_collection_info(collection).await?;
        let mut topo = VectorTopology {
            dense_vector: None,
            sparse_vector: None,
            rerank_vector: None,
        };

        for vname in &info.schema.dense_vectors {
            if vname == crate::executor::DENSE_VECTOR_NAME {
                topo.dense_vector = Some(crate::executor::DENSE_VECTOR_NAME.to_string());
            } else if vname == crate::executor::RERANK_VECTOR_NAME {
                topo.rerank_vector = Some(crate::executor::RERANK_VECTOR_NAME.to_string());
            } else if topo.dense_vector.is_none()
                || topo
                    .dense_vector
                    .as_ref()
                    .is_some_and(|name| name.is_empty())
            {
                topo.dense_vector = Some(vname.clone());
            }
        }

        for vname in &info.schema.sparse_vectors {
            if vname == crate::executor::SPARSE_VECTOR_NAME {
                topo.sparse_vector = Some(crate::executor::SPARSE_VECTOR_NAME.to_string());
            } else if topo.sparse_vector.is_none()
                || topo
                    .sparse_vector
                    .as_ref()
                    .is_some_and(|name| name.is_empty())
            {
                topo.sparse_vector = Some(vname.clone());
            }
        }

        Ok(topo)
    }

    pub(crate) async fn resolve_dense_vector_size(
        &self,
        model: Option<&str>,
    ) -> Result<usize, QqlError> {
        if self.uses_local_embeddings() {
            if let Some(ref cfg) = self.config {
                if cfg.embedding_dimension > 0 {
                    return Ok(cfg.embedding_dimension);
                }
            }
            return match self.config.as_ref() {
                #[cfg(feature = "rest")]
                Some(cfg)
                    if !cfg.embedding_endpoint.as_deref().unwrap_or("").is_empty()
                        && !cfg.embedding_model.as_deref().unwrap_or("").is_empty() =>
                {
                    let embedder = HttpEmbedder::new(
                        cfg.embedding_endpoint.clone().unwrap_or_default(),
                        cfg.embedding_api_key.clone().unwrap_or_default(),
                        cfg.embedding_model.clone().unwrap_or_default(),
                        1,
                    )?;
                    let dim = embedder.probe_dimension("probe").await?;
                    Ok(dim)
                }
                _ if model.is_none() => Ok(crate::executor::DENSE_VECTOR_SIZE as usize),
                _ => Err(QqlError::runtime(
                    "embedding_dimension must be configured when creating collections with USING MODEL in local inference mode",
                )),
            };
        }

        if let Some(ref cfg) = self.config {
            if cfg.embedding_dimension > 0 {
                return Ok(cfg.embedding_dimension);
            }
        }

        if model.is_some()
            && model.unwrap() != ""
            && self
                .config
                .as_ref()
                .map(|c| c.embedding_dimension == 0)
                .unwrap_or(true)
        {
            return Err(QqlError::runtime(
                "embedding_dimension must be configured when creating collections with USING MODEL",
            ));
        }

        Ok(crate::executor::DENSE_VECTOR_SIZE as usize)
    }
}
