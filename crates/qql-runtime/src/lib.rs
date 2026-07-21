pub mod backend;
pub mod client;
pub mod config;
pub mod embedder;
pub mod executor;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "grpc")]
mod grpc_route;
pub mod qdrant;
#[cfg(feature = "grpc")]
pub mod qdrant_grpc;
#[cfg(feature = "rest")]
pub mod rest;
pub mod sparse;

#[cfg(test)]
mod sparse_test;

#[cfg(test)]
mod contract_test;
#[cfg(test)]
mod executor_test;
