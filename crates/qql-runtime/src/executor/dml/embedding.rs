use crate::embedder::Embedder;
use crate::executor::Executor;
use qql_core::ast::{
    EmbedKind, EmbeddingSpec, PointVectors, QueryExpr, QueryInput, QueryStmt, Stmt, UpsertPoint,
    UpsertStmt, VectorValue,
};
use qql_core::error::QqlError;

impl Executor {
    pub async fn resolve_embeddings(
        &self,
        stmt: &mut Stmt,
        embedder: &dyn Embedder,
    ) -> Result<(), QqlError> {
        match stmt {
            Stmt::Query(query) => {
                Self::resolve_query_embeddings(query, embedder).await?;
            }
            Stmt::Upsert(upsert) => {
                Self::resolve_upsert_embeddings(upsert, embedder).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn resolve_query_embeddings(
        query: &mut QueryStmt,
        embedder: &dyn Embedder,
    ) -> Result<(), QqlError> {
        Self::resolve_query_expr_embeddings(&mut query.expression, embedder).await?;
        for cte in &mut query.ctes {
            Self::resolve_query_expr_embeddings(&mut cte.query.expression, embedder).await?;
        }
        Ok(())
    }

    fn resolve_prefetches<'a>(
        prefetches: &'a mut [qql_core::ast::Prefetch],
        embedder: &'a dyn Embedder,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), QqlError>> + Send + 'a>> {
        Box::pin(async move {
            for pref in prefetches {
                if let qql_core::ast::PrefetchSource::Query(ref mut sub_query) = pref.source {
                    Self::resolve_query_embeddings(sub_query, embedder).await?;
                }
            }
            Ok(())
        })
    }

    async fn resolve_query_expr_embeddings(
        expr: &mut QueryExpr,
        embedder: &dyn Embedder,
    ) -> Result<(), QqlError> {
        match expr {
            QueryExpr::Nearest { input, using, prefetch, .. } => {
                let v_name = using.as_deref().unwrap_or("default");
                Self::resolve_query_input(input, embedder, v_name).await?;
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::Recommend {
                positive,
                negative,
                using,
                prefetch,
                ..
            } => {
                let v_name = using.as_deref().unwrap_or("default");
                for pos in positive {
                    Self::resolve_query_input(pos, embedder, v_name).await?;
                }
                for neg in negative {
                    Self::resolve_query_input(neg, embedder, v_name).await?;
                }
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::Context { pairs, using, prefetch, .. } => {
                let v_name = using.as_deref().unwrap_or("default");
                for pair in pairs {
                    Self::resolve_query_input(&mut pair.positive, embedder, v_name).await?;
                    Self::resolve_query_input(&mut pair.negative, embedder, v_name).await?;
                }
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::Discover {
                target,
                context,
                using,
                prefetch,
                ..
            } => {
                let v_name = using.as_deref().unwrap_or("default");
                Self::resolve_query_input(target, embedder, v_name).await?;
                for pair in context {
                    Self::resolve_query_input(&mut pair.positive, embedder, v_name).await?;
                    Self::resolve_query_input(&mut pair.negative, embedder, v_name).await?;
                }
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::Fusion { prefetch, .. } => {
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::Formula { prefetch, .. } => {
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::RelevanceFeedback {
                target,
                feedback,
                using,
                prefetch,
                ..
            } => {
                let v_name = using.as_deref().unwrap_or("default");
                Self::resolve_query_input(target, embedder, v_name).await?;
                for fb in feedback {
                    Self::resolve_query_input(&mut fb.example, embedder, v_name).await?;
                }
                Self::resolve_prefetches(prefetch, embedder).await?;
            }
            QueryExpr::Hybrid {
                text,
                model,
                dense_vector,
                sparse_vector,
                fusion,
            } => {
                let m_name = model.as_deref().unwrap_or("default");
                let d_vec_name = dense_vector
                    .as_deref()
                    .unwrap_or(crate::executor::DENSE_VECTOR_NAME);
                let s_vec_name = sparse_vector
                    .as_deref()
                    .unwrap_or(crate::executor::SPARSE_VECTOR_NAME);

                let d_vec = embedder.embed_dense(text, m_name).await?;
                let s_vec = embedder.embed_sparse(text).await?;

                let dense_sub_query = qql_core::ast::QueryStmt {
                    ctes: Vec::new(),
                    collection: qql_core::ast::QueryCollection::Inherited,
                    expression: QueryExpr::Nearest {
                        input: QueryInput::Vector(VectorValue::Dense(d_vec)),
                        using: Some(d_vec_name.to_string()),
                        prefetch: Vec::new(),
                        mmr: None,
                    },
                    filter: None,
                    params: None,
                    score_threshold: None,
                    group: None,
                    output: qql_core::ast::QueryOutput::default(),
                    page: qql_core::ast::PageSpec::default(),
                };

                let sparse_sub_query = qql_core::ast::QueryStmt {
                    ctes: Vec::new(),
                    collection: qql_core::ast::QueryCollection::Inherited,
                    expression: QueryExpr::Nearest {
                        input: QueryInput::Vector(VectorValue::Sparse {
                            indices: s_vec.indices,
                            values: s_vec.values,
                        }),
                        using: Some(s_vec_name.to_string()),
                        prefetch: Vec::new(),
                        mmr: None,
                    },
                    filter: None,
                    params: None,
                    score_threshold: None,
                    group: None,
                    output: qql_core::ast::QueryOutput::default(),
                    page: qql_core::ast::PageSpec::default(),
                };

                let prefetches = vec![
                    qql_core::ast::Prefetch {
                        source: qql_core::ast::PrefetchSource::Query(Box::new(dense_sub_query)),
                        filter: None,
                        score_threshold: None,
                        lookup: None,
                    },
                    qql_core::ast::Prefetch {
                        source: qql_core::ast::PrefetchSource::Query(Box::new(sparse_sub_query)),
                        filter: None,
                        score_threshold: None,
                        lookup: None,
                    },
                ];

                *expr = QueryExpr::Fusion {
                    method: *fusion,
                    prefetch: prefetches,
                };
            }
            _ => {}
        }
        Ok(())
    }

    async fn resolve_query_input(
        input: &mut QueryInput,
        embedder: &dyn Embedder,
        vector_or_model: &str,
    ) -> Result<(), QqlError> {
        if let QueryInput::Text { text, model } = input {
            if vector_or_model == "sparse" {
                let s_vec = embedder.embed_sparse(text).await?;
                *input = QueryInput::Vector(VectorValue::Sparse {
                    indices: s_vec.indices,
                    values: s_vec.values,
                });
            } else {
                let model_name = model.as_deref().unwrap_or(vector_or_model);
                let vec = embedder.embed_dense(text, model_name).await?;
                *input = QueryInput::Vector(VectorValue::Dense(vec));
            }
        }
        Ok(())
    }

    async fn resolve_upsert_embeddings(
        upsert: &mut UpsertStmt,
        embedder: &dyn Embedder,
    ) -> Result<(), QqlError> {
        if upsert.embedding.is_none() && upsert.embed.is_empty() {
            let mut targets = Vec::new();
            for (idx, point) in upsert.points.iter().enumerate() {
                if point.vectors.is_none() {
                    if let Some((_, qql_core::ast::Value::Str(text))) = point
                        .payload
                        .iter()
                        .find(|(k, _)| {
                            k.eq_ignore_ascii_case("text")
                                || k.eq_ignore_ascii_case("body")
                                || k.eq_ignore_ascii_case("content")
                        })
                    {
                        if !text.is_empty() {
                            targets.push((idx, text.clone()));
                        }
                    }
                }
            }
            if !targets.is_empty() {
                let texts: Vec<String> = targets.iter().map(|(_, t)| t.clone()).collect();
                let dense_vecs = embedder.embed_dense_batch(&texts, "default").await?;
                for ((idx, text), d_vec) in targets.into_iter().zip(dense_vecs) {
                    let point = &mut upsert.points[idx];
                    add_point_vector(
                        point,
                        crate::executor::DENSE_VECTOR_NAME,
                        VectorValue::Dense(d_vec),
                    );
                    if let Ok(sparse_vec) = embedder.embed_sparse(&text).await {
                        add_point_vector(
                            point,
                            crate::executor::SPARSE_VECTOR_NAME,
                            VectorValue::Sparse {
                                indices: sparse_vec.indices,
                                values: sparse_vec.values,
                            },
                        );
                    }
                }
            }
        }

        if let Some(ref spec) = upsert.embedding {
            match spec {
                EmbeddingSpec::Dense { model, vector } => {
                    let model_name = model.as_deref().unwrap_or("default");
                    let vector_name =
                        vector.as_deref().unwrap_or(crate::executor::DENSE_VECTOR_NAME);

                    let mut targets = Vec::new();
                    for (idx, point) in upsert.points.iter().enumerate() {
                        if let Some((_, qql_core::ast::Value::Str(text))) = point
                            .payload
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("text"))
                        {
                            if !text.is_empty() {
                                targets.push((idx, text.clone()));
                            }
                        }
                    }

                    if !targets.is_empty() {
                        let texts: Vec<String> = targets.iter().map(|(_, t)| t.clone()).collect();
                        let vecs = embedder.embed_dense_batch(&texts, model_name).await?;
                        for ((idx, _), vec) in targets.into_iter().zip(vecs) {
                            let point = &mut upsert.points[idx];
                            add_point_vector(point, vector_name, VectorValue::Dense(vec));
                        }
                    }
                }
                EmbeddingSpec::Hybrid {
                    dense_model,
                    dense_vector,
                    sparse_vector,
                    ..
                } => {
                    let d_model = dense_model.as_deref().unwrap_or("default");
                    let d_vec_name =
                        dense_vector.as_deref().unwrap_or(crate::executor::DENSE_VECTOR_NAME);
                    let s_vec_name =
                        sparse_vector.as_deref().unwrap_or(crate::executor::SPARSE_VECTOR_NAME);

                    let mut targets = Vec::new();
                    for (idx, point) in upsert.points.iter().enumerate() {
                        if let Some((_, qql_core::ast::Value::Str(text))) = point
                            .payload
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("text"))
                        {
                            if !text.is_empty() {
                                targets.push((idx, text.clone()));
                            }
                        }
                    }

                    if !targets.is_empty() {
                        let texts: Vec<String> = targets.iter().map(|(_, t)| t.clone()).collect();
                        let dense_vecs = embedder.embed_dense_batch(&texts, d_model).await?;
                        for ((idx, text), d_vec) in targets.into_iter().zip(dense_vecs) {
                            let sparse_vec = embedder.embed_sparse(&text).await?;
                            let point = &mut upsert.points[idx];
                            add_point_vector(point, d_vec_name, VectorValue::Dense(d_vec));
                            add_point_vector(
                                point,
                                s_vec_name,
                                VectorValue::Sparse {
                                    indices: sparse_vec.indices,
                                    values: sparse_vec.values,
                                },
                            );
                        }
                    }
                }
            }
        }

        for directive in &upsert.embed {
            let field_name = &directive.source_field;
            let target_vec_name = &directive.target_vector;
            let mut targets = Vec::new();
            for (idx, point) in upsert.points.iter().enumerate() {
                if let Some((_, qql_core::ast::Value::Str(text))) = point
                    .payload
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(field_name))
                {
                    if !text.is_empty() {
                        targets.push((idx, text.clone()));
                    }
                }
            }

            if !targets.is_empty() {
                match &directive.kind {
                    EmbedKind::Dense { model } => {
                        let m_name = model.as_deref().unwrap_or("default");
                        let texts: Vec<String> = targets.iter().map(|(_, t)| t.clone()).collect();
                        let vecs = embedder.embed_dense_batch(&texts, m_name).await?;
                        for ((idx, _), vec) in targets.into_iter().zip(vecs) {
                            let point = &mut upsert.points[idx];
                            add_point_vector(point, target_vec_name, VectorValue::Dense(vec));
                        }
                    }
                    EmbedKind::Sparse { .. } => {
                        for (idx, text) in targets {
                            let s_vec = embedder.embed_sparse(&text).await?;
                            let point = &mut upsert.points[idx];
                            add_point_vector(
                                point,
                                target_vec_name,
                                VectorValue::Sparse {
                                    indices: s_vec.indices,
                                    values: s_vec.values,
                                },
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

fn add_point_vector(point: &mut UpsertPoint, name: &str, vector: VectorValue) {
    match &mut point.vectors {
        Some(PointVectors::Named(list)) => {
            if let Some(existing) = list.iter_mut().find(|(k, _)| k == name) {
                existing.1 = vector;
            } else {
                list.push((name.to_string(), vector));
            }
        }
        Some(PointVectors::Unnamed(_)) | None => {
            point.vectors = Some(PointVectors::Named(vec![(name.to_string(), vector)]));
        }
    }
}
