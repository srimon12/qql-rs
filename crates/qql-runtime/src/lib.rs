pub mod backend;
pub mod client;
pub mod config;
pub mod embedder;
pub mod executor;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod pipeline;
pub mod qdrant;
#[cfg(feature = "rest")]
pub mod rest;
pub mod sparse;

#[cfg(test)]
mod pipeline_test;
#[cfg(test)]
mod sparse_test;

#[cfg(test)]
mod contract_test;
#[cfg(test)]
mod executor_test;
