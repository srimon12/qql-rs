//! Embedding resolution for the executor — delegates to `qql-embed`.

use crate::embedder::Embedder;
use crate::executor::Executor;
use qql_core::ast::Stmt;
use qql_core::error::QqlError;

impl Executor {
    /// Resolve text → vectors on the statement AST (batched dense via the embedder).
    pub async fn resolve_embeddings(
        &self,
        stmt: &mut Stmt,
        embedder: &dyn Embedder,
    ) -> Result<(), QqlError> {
        qql_embed::resolve_embeddings(stmt, embedder).await
    }
}
