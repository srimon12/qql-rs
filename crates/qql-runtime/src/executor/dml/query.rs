use crate::client::CollectionInfo;
use crate::executor::{Executor, SearchHit};
use qql_core::ast::{
    Prefetch, PrefetchSource, QueryExpr, QueryInput, QueryStmt, VectorKind, VectorTarget,
    VectorValue,
};
use qql_core::error::QqlError;

impl Executor {
    /// Resolve omitted vector names from the collection schema and validate
    /// explicit names before text is embedded.
    pub(crate) async fn configure_query_vectors(
        &self,
        collection: &str,
        query: &mut QueryStmt,
    ) -> Result<(), QqlError> {
        if !query_requires_schema(query) {
            return Ok(());
        }
        let info = self.client.get_collection_info(collection).await?;
        let topology = QueryTopology::from_info(&info);
        configure_query(collection, query, &topology)
    }
}

fn query_requires_schema(query: &QueryStmt) -> bool {
    query
        .ctes
        .iter()
        .any(|cte| query_requires_schema(&cte.query))
        || expression_requires_schema(&query.expression)
}

fn expression_requires_schema(expression: &QueryExpr) -> bool {
    match expression {
        QueryExpr::Nearest {
            using, prefetch, ..
        }
        | QueryExpr::Recommend {
            using, prefetch, ..
        }
        | QueryExpr::Context {
            using, prefetch, ..
        }
        | QueryExpr::Discover {
            using, prefetch, ..
        }
        | QueryExpr::RelevanceFeedback {
            using, prefetch, ..
        } => target_requires_schema(using) || prefetch.iter().any(prefetch_requires_schema),
        QueryExpr::Rerank {
            using, prefetch, ..
        } => target_requires_schema(using) || prefetch.iter().any(prefetch_requires_schema),
        QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
            prefetch.iter().any(prefetch_requires_schema)
        }
        QueryExpr::Hybrid {
            dense_vector,
            sparse_vector,
            ..
        } => dense_vector.is_none() || sparse_vector.is_none(),
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => false,
    }
}

fn target_requires_schema(target: &Option<VectorTarget>) -> bool {
    target.as_ref().is_none_or(|target| target.kind.is_none())
}

fn prefetch_requires_schema(prefetch: &Prefetch) -> bool {
    match &prefetch.source {
        PrefetchSource::Cte(_) => false,
        PrefetchSource::Query(query) => query_requires_schema(query),
    }
}

#[derive(Debug)]
struct QueryTopology {
    dense: Vec<String>,
    sparse: Vec<String>,
}

impl QueryTopology {
    fn from_info(info: &CollectionInfo) -> Self {
        let mut dense = if info.schema.vectors.is_empty() {
            info.schema.dense_vectors.clone()
        } else {
            info.schema
                .vectors
                .iter()
                .map(|vector| vector.name.clone().unwrap_or_default())
                .collect()
        };
        let sparse: Vec<String> = info
            .schema
            .sparse_vectors
            .iter()
            .map(|vector| vector.name.clone())
            .collect();
        if dense.is_empty() && sparse.is_empty() {
            // An empty schema is Qdrant's unnamed default dense vector. This
            // also keeps custom backends that expose only collection counts
            // compatible with the default QUERY text form.
            dense.push(String::new());
        }
        Self { dense, sparse }
    }

    fn all(&self) -> impl Iterator<Item = &str> {
        self.dense.iter().chain(&self.sparse).map(String::as_str)
    }

    fn select(&self, kind: Option<VectorKind>) -> Option<(&str, VectorKind)> {
        let candidates = match kind {
            Some(VectorKind::Dense) => &self.dense,
            Some(VectorKind::Sparse) => &self.sparse,
            None => {
                if self.dense.len() + self.sparse.len() == 1 {
                    return self
                        .dense
                        .first()
                        .map(|name| (name.as_str(), VectorKind::Dense))
                        .or_else(|| {
                            self.sparse
                                .first()
                                .map(|name| (name.as_str(), VectorKind::Sparse))
                        });
                }
                return None;
            }
        };
        (candidates.len() == 1).then(|| {
            (
                candidates[0].as_str(),
                kind.expect("typed candidate selection has a vector kind"),
            )
        })
    }

    fn kind_of(&self, name: &str) -> Option<VectorKind> {
        if self.dense.iter().any(|candidate| candidate == name) {
            Some(VectorKind::Dense)
        } else if self.sparse.iter().any(|candidate| candidate == name) {
            Some(VectorKind::Sparse)
        } else {
            None
        }
    }
}

fn configure_query(
    collection: &str,
    query: &mut QueryStmt,
    topology: &QueryTopology,
) -> Result<(), QqlError> {
    for cte in &mut query.ctes {
        configure_query(collection, &mut cte.query, topology)?;
    }
    configure_expr(collection, &mut query.expression, topology)
}

fn configure_expr(
    collection: &str,
    expression: &mut QueryExpr,
    topology: &QueryTopology,
) -> Result<(), QqlError> {
    match expression {
        QueryExpr::Nearest {
            input,
            using,
            prefetch,
            ..
        } => {
            resolve_using(collection, using, input_kind(input), topology)?;
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::Recommend {
            positive,
            negative,
            using,
            prefetch,
            ..
        } => {
            let kind = merge_input_kinds(positive.iter().chain(negative.iter()))?;
            resolve_using(collection, using, kind, topology)?;
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::Context {
            pairs,
            using,
            prefetch,
        } => {
            let kind = merge_input_kinds(
                pairs
                    .iter()
                    .flat_map(|pair| [&pair.positive, &pair.negative]),
            )?;
            resolve_using(collection, using, kind, topology)?;
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::Discover {
            target,
            context,
            using,
            prefetch,
        } => {
            let kind = merge_input_kinds(
                core::iter::once(&*target).chain(
                    context
                        .iter()
                        .flat_map(|pair| [&pair.positive, &pair.negative]),
                ),
            )?;
            resolve_using(collection, using, kind, topology)?;
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            using,
            prefetch,
            ..
        } => {
            let kind = merge_input_kinds(
                core::iter::once(&*target).chain(feedback.iter().map(|item| &item.example)),
            )?;
            resolve_using(collection, using, kind, topology)?;
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::Rerank {
            using, prefetch, ..
        } => {
            resolve_using(collection, using, Some(VectorKind::Dense), topology)?;
            configure_prefetches(collection, prefetch, topology)
        }
        QueryExpr::Hybrid {
            dense_vector,
            sparse_vector,
            ..
        } => {
            resolve_required_vector(collection, dense_vector, &topology.dense, "dense")?;
            resolve_required_vector(collection, sparse_vector, &topology.sparse, "sparse")
        }
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => Ok(()),
    }
}

fn configure_prefetches(
    collection: &str,
    prefetches: &mut [Prefetch],
    topology: &QueryTopology,
) -> Result<(), QqlError> {
    for prefetch in prefetches {
        if let PrefetchSource::Query(query) = &mut prefetch.source {
            configure_query(collection, query, topology)?;
        }
    }
    Ok(())
}

fn input_kind(input: &QueryInput) -> Option<VectorKind> {
    match input {
        QueryInput::Text { .. } | QueryInput::Point(_) => None,
        QueryInput::Vector(VectorValue::Dense(_) | VectorValue::MultiDense(_)) => {
            Some(VectorKind::Dense)
        }
        QueryInput::Vector(VectorValue::Sparse { .. }) => Some(VectorKind::Sparse),
    }
}

fn merge_input_kinds<'a>(
    inputs: impl IntoIterator<Item = &'a QueryInput>,
) -> Result<Option<VectorKind>, QqlError> {
    let mut resolved = None;
    for input in inputs {
        let Some(kind) = input_kind(input) else {
            continue;
        };
        if resolved.is_some_and(|current| current != kind) {
            return Err(QqlError::validation(
                "QQL-VALIDATION-VECTOR-KIND",
                "query inputs cannot mix dense and sparse vector values",
                None,
            ));
        }
        resolved = Some(kind);
    }
    Ok(resolved)
}

fn resolve_using(
    collection: &str,
    using: &mut Option<VectorTarget>,
    required_kind: Option<VectorKind>,
    topology: &QueryTopology,
) -> Result<(), QqlError> {
    if let Some(target) = using {
        let actual_kind = topology.kind_of(&target.name).ok_or_else(|| {
            let available: Vec<String> = topology.all().map(str::to_string).collect();
            unknown_vector_error(collection, &target.name, &available)
        })?;
        if target.kind.is_some_and(|hint| hint != actual_kind)
            || required_kind.is_some_and(|required| required != actual_kind)
        {
            return Err(vector_kind_error(collection, &target.name, actual_kind));
        }
        target.kind = Some(actual_kind);
        return Ok(());
    }
    let Some((name, kind)) = topology.select(required_kind) else {
        return Err(missing_using_error(collection, topology));
    };
    if !name.is_empty() {
        *using = Some(VectorTarget {
            name: name.to_string(),
            kind: Some(kind),
        });
    }
    Ok(())
}

fn resolve_required_vector(
    collection: &str,
    vector: &mut Option<String>,
    available: &[String],
    kind: &str,
) -> Result<(), QqlError> {
    if let Some(name) = vector.as_deref() {
        if available.iter().any(|candidate| candidate == name) {
            return Ok(());
        }
        return Err(unknown_vector_error(collection, name, available));
    }
    if available.len() != 1 || available[0].is_empty() {
        return Err(QqlError::execution(
            "QQL-MISSING-USING",
            format!(
                "Collection '{collection}' does not have exactly one named {kind} vector. Specify the {kind} vector explicitly. Available {kind} vectors: {}",
                display_names(available)
            ),
            None,
        ));
    }
    *vector = Some(available[0].clone());
    Ok(())
}

fn missing_using_error(collection: &str, topology: &QueryTopology) -> QqlError {
    let names: Vec<String> = topology.all().map(str::to_string).collect();
    QqlError::execution(
        "QQL-MISSING-USING",
        format!(
            "Collection '{collection}' has an ambiguous vector topology. Add USING <vector_name>. Available vectors: {}",
            display_names(&names)
        ),
        None,
    )
}

fn vector_kind_error(collection: &str, name: &str, actual: VectorKind) -> QqlError {
    QqlError::execution(
        "QQL-VECTOR-KIND",
        format!(
            "Vector '{name}' in collection '{collection}' is {}, which is incompatible with this query",
            match actual {
                VectorKind::Dense => "dense",
                VectorKind::Sparse => "sparse",
            }
        ),
        None,
    )
}

fn unknown_vector_error(collection: &str, name: &str, available: &[String]) -> QqlError {
    QqlError::execution(
        "QQL-UNKNOWN-VECTOR",
        format!(
            "Collection '{collection}' has no vector named '{name}'. Available vectors: {}",
            display_names(available)
        ),
        None,
    )
}

fn display_names(names: &[String]) -> String {
    names
        .iter()
        .map(|name| if name.is_empty() { "<default>" } else { name })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn extract_search_hits(result: &serde_json::Value) -> Vec<SearchHit> {
    let points = result
        .get("result")
        .and_then(|r| r.get("points"))
        .and_then(serde_json::Value::as_array)
        .or_else(|| result.get("points").and_then(serde_json::Value::as_array))
        .or_else(|| result.get("result").and_then(serde_json::Value::as_array));

    match points {
        Some(pts) => pts
            .iter()
            .map(|hit| SearchHit {
                id: hit
                    .get("id")
                    .map(|id| match id {
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => id.to_string(),
                    })
                    .unwrap_or_default(),
                score: hit
                    .get("score")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0) as f32,
                text: hit
                    .get("payload")
                    .and_then(|p| p.get("text"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                payload: hit.get("payload").and_then(|p| {
                    p.as_object()
                        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                }),
            })
            .collect(),
        None => Vec::new(),
    }
}
