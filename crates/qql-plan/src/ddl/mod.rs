//! DDL lowering: collection/index/shard/quota statements to plan requests.
//!
//! Split by size hygiene: [`collection`] (create/alter + fill), [`index`]
//! (create-index options), [`diff`] (alter diffs), [`options`] (WAL,
//! strict-mode, metadata, replica states), [`runtime`] (HNSW/optimizer/
//! quantization IR), [`quota`]. Stable paths are re-exported here so
//! `plan.rs`, `routing.rs`, and the transport adapters keep their imports.

mod collection;
mod diff;
mod index;
mod options;
mod quota;
mod runtime;

#[cfg(test)]
mod tests;

pub use crate::ddl_rest::{
    CreateCollectionDeferredParams, CreateCollectionRestBody, CreateIndexRestBody, RestDdlStep,
    create_collection_deferred_params_rest, create_collection_rest_body,
    create_collection_rest_steps, create_index_op, create_index_rest_body, drop_index_op,
    update_collection_op,
};

pub use collection::{lower_alter_collection, lower_create_collection};
pub use index::lower_create_index;
pub use runtime::{lower_hnsw_config, lower_optimizers_config, lower_quantization_config};

pub(crate) use options::lower_replica_state;
pub(crate) use quota::lower_set_quota;

use qql_core::error::{QqlError, Span};

fn collection_config_error(
    message: impl Into<alloc::borrow::Cow<'static, str>>,
    span: Option<Span>,
) -> QqlError {
    QqlError::validation("QQL-PLAN-COLLECTION-CONFIG", message, span)
}
