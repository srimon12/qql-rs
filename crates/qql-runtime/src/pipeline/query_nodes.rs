use async_trait::async_trait;
use qql_core::ast;
use qql_core::error::QqlError;

use super::{
    ContextInput, ContextPair, DiscoverInput, ExecutionNode, FeedbackItem, FusionType,
    NaiveFeedbackStrategy, OrderByDirection, OrderByInput, QueryState, QueryVariant,
    RecommendInput, RecommendStrategyType, RelevanceFeedbackInput, VectorInput,
};

pub struct RecommendNode {
    pub positive_ids: Vec<ast::Value>,
    pub negative_ids: Vec<ast::Value>,
    pub strategy: Option<String>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for RecommendNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if state.has_mmr {
            return Err(QqlError::runtime(
                "MMR is supported only for standard NEAREST queries",
            ));
        }
        if self.positive_ids.is_empty() && self.negative_ids.is_empty() {
            return Err(QqlError::runtime(
                "RECOMMEND requires at least one POSITIVE or NEGATIVE ID",
            ));
        }

        let mut pos = Vec::new();
        for id in &self.positive_ids {
            let vi = super::helpers::build_vector_input(state, id).await?;
            pos.push(vi);
        }

        let mut neg = Vec::new();
        for id in &self.negative_ids {
            let vi = super::helpers::build_vector_input(state, id).await?;
            neg.push(vi);
        }

        let strategy = self
            .strategy
            .as_ref()
            .and_then(|s| RecommendStrategyType::parse(s));

        let rec = RecommendInput {
            positive: pos,
            negative: neg,
            strategy,
        };

        state.target_query = Some(QueryVariant::Recommend(rec));
        Ok(())
    }
}

pub struct ContextPairInput {
    pub positive: Option<ast::Value>,
    pub negative: Option<ast::Value>,
}

pub struct ContextNode {
    pub pairs: Vec<ContextPairInput>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for ContextNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let pairs = build_context_pairs(state, &self.pairs).await?;
        state.target_query = Some(QueryVariant::Context(ContextInput { pairs }));
        Ok(())
    }
}

pub struct DiscoverNode {
    pub target: Option<ast::Value>,
    pub pairs: Vec<ContextPairInput>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for DiscoverNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let target = match &self.target {
            Some(v) => super::helpers::build_vector_input(state, v).await?,
            None => return Err(QqlError::runtime("DISCOVER requires a target")),
        };
        let pairs = build_context_pairs(state, &self.pairs).await?;
        state.target_query = Some(QueryVariant::Discover(DiscoverInput {
            target,
            context: ContextInput { pairs },
        }));
        Ok(())
    }
}

async fn build_context_pairs(
    state: &QueryState,
    pairs: &[ContextPairInput],
) -> Result<Vec<ContextPair>, QqlError> {
    let mut result = Vec::with_capacity(pairs.len());
    for p in pairs {
        let positive = match &p.positive {
            Some(v) => Some(super::helpers::build_vector_input(state, v).await?),
            None => None,
        };
        let negative = match &p.negative {
            Some(v) => Some(super::helpers::build_vector_input(state, v).await?),
            None => None,
        };
        result.push(ContextPair { positive, negative });
    }
    Ok(result)
}

pub struct OrderByNode {
    pub field: String,
    pub asc: bool,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for OrderByNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let direction = if self.asc {
            OrderByDirection::Asc
        } else {
            OrderByDirection::Desc
        };
        state.target_query = Some(QueryVariant::OrderBy(OrderByInput {
            key: self.field.clone(),
            direction,
        }));
        Ok(())
    }
}

pub struct SampleNode;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for SampleNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        state.target_query = Some(QueryVariant::Sample);
        Ok(())
    }
}

pub struct RelevanceFeedbackNode {
    pub target: ast::Value,
    pub feedback: Vec<(ast::Value, f64)>,
    pub strategy: Option<(f64, f64, f64)>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for RelevanceFeedbackNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        fn build_vector_input_from_value(val: &ast::Value) -> Result<VectorInput, QqlError> {
            match val {
                ast::Value::List(items) => {
                    let vec: Vec<f32> = items
                        .iter()
                        .map(|v| match v {
                            ast::Value::Float(f) => Ok(*f as f32),
                            ast::Value::Int(i) => Ok(*i as f32),
                            _ => Err(QqlError::runtime("vector element is not a number")),
                        })
                        .collect::<Result<Vec<f32>, QqlError>>()?;
                    Ok(VectorInput::Dense(vec))
                }
                _ => {
                    let pid = super::helpers::to_point_id(val)?;
                    Ok(VectorInput::Id(pid))
                }
            }
        }

        let target_input = build_vector_input_from_value(&self.target)
            .map_err(|e| QqlError::runtime(format!("relevance feedback target: {}", e)))?;

        let mut feedback_items = Vec::with_capacity(self.feedback.len());
        for (i, (example, score)) in self.feedback.iter().enumerate() {
            let example_input = build_vector_input_from_value(example).map_err(|e| {
                QqlError::runtime(format!("relevance feedback example {}: {}", i, e))
            })?;
            feedback_items.push(FeedbackItem {
                example: example_input,
                score: *score as f32,
            });
        }

        let strategy = self.strategy.map(|(a, b, c)| NaiveFeedbackStrategy {
            a: a as f32,
            b: b as f32,
            c: c as f32,
        });

        state.target_query = Some(QueryVariant::RelevanceFeedback(RelevanceFeedbackInput {
            target: target_input,
            feedback: feedback_items,
            strategy,
        }));

        Ok(())
    }
}

pub struct FusionNode {
    pub mode: String,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for FusionNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let mode_lower = self.mode.to_lowercase();
        if mode_lower == "rrf" {
            if let Some(config) = &state.fusion_config {
                state.target_query = Some(QueryVariant::Rrf(config.clone()));
                return Ok(());
            }
        }

        match mode_lower.as_str() {
            "rrf" => {
                state.target_query = Some(QueryVariant::Fusion(FusionType::Rrf));
            }
            "dbsf" => {
                state.target_query = Some(QueryVariant::Fusion(FusionType::Dbsf));
            }
            _ => {
                return Err(QqlError::runtime(format!(
                    "unknown fusion mode '{}'; expected 'rrf' or 'dbsf'",
                    self.mode
                )));
            }
        }

        Ok(())
    }
}

pub struct RerankNode {
    pub model: String,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for RerankNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if state.local_embed {
            return Err(QqlError::runtime(
                "RERANK is currently only available in cloud inference mode",
            ));
        }

        state.target_query = Some(QueryVariant::Document {
            text: state.query_text.clone(),
            model: self.model.clone(),
            options: state.get_doc_options(),
        });

        Ok(())
    }
}
