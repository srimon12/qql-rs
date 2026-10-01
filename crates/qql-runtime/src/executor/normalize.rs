//! Response normalization, re-exported from
//! [`qql_protocol::normalize`].
//!
//! `normalize_planned` and the batch item normalizers live in `qql-protocol`
//! so the runtime executor and the WASM host report the same labels,
//! messages, and closed payloads. **No I/O here**: server telemetry travels
//! on the response, never on the error path.

pub(crate) use qql_protocol::normalize::{
    normalize_planned, normalize_query_item, normalize_update_item,
};
