//! Typify-generated REST types from `openapi.json`.
//!
//! These types mirror Qdrant's OpenAPI component schemas for REST wire shapes.
//! They are generated at build time; do not edit the included file by hand.

#![allow(clippy::all)]
#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused_imports)]
// Generated OpenAPI comments can still mention server-only concepts; keep the
// module documentable under `RUSTDOCFLAGS=-D warnings` even if a future schema
// reintroduces a link we do not neutralize.
#![allow(rustdoc::broken_intra_doc_links)]
#![allow(rustdoc::private_intra_doc_links)]

include!(concat!(env!("OUT_DIR"), "/qdrant_types.rs"));
