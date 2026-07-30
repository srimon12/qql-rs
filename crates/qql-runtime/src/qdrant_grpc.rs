//! Tonic-generated gRPC types from `proto/`.
//!
//! Protobuf messages and clients for the Qdrant gRPC API. Generated at build
//! time when the `grpc` feature is enabled.

#![allow(clippy::all)]
#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused_imports)]
#![allow(rustdoc::broken_intra_doc_links)]
#![allow(rustdoc::private_intra_doc_links)]

pub mod qdrant {
    tonic::include_proto!("qdrant");
}
