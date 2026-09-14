//! `BATCH { … }` statement: one Qdrant batch RPC for homogeneous members.

use alloc::vec::Vec;

use super::{SearchParams, Stmt};

/// `BATCH { <stmt>; … }` — members execute as one batch RPC.
///
/// Members are full statements sharing one collection and one family (all
/// queries or all mutations). Homogeneity is validated by the lowerer; the
/// parser only rejects empty blocks, nested batches, and non-batchable
/// statement kinds (DDL, `SHOW`, `COUNT`, `SCROLL`, `FACET`, `SET QUOTA`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BatchStmt {
    /// Member statements in execution order.
    pub statements: Vec<Stmt>,
    /// Durability flag for mutation batches (`WAIT`, `?wait=`).
    pub wait: Option<bool>,
    /// Shared read opts for query batches (`PARAMS`, `?timeout=`/`?consistency=`).
    pub params: Option<SearchParams>,
}
