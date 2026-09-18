#![allow(clippy::field_reassign_with_default)]

mod batch;
#[cfg(feature = "rest")]
mod bm25;
mod ddl;
mod estimate;
pub(crate) mod mock;
mod prepared;
mod query;
mod response;
mod telemetry;
mod typed_hits;
mod upsert;
