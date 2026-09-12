//! CLI command handlers.

mod check;
mod convert;
mod doctor;
mod dump_migrate;
mod edge;
mod exec;
mod runtime;
mod version;

pub use check::handle_check;
#[cfg(test)]
pub(crate) use convert::source_is_canonical;
pub use convert::{handle_convert, handle_fmt};
pub use doctor::handle_doctor;
pub use dump_migrate::{handle_dump, handle_migrate};
pub use edge::handle_configure_edge;
#[cfg(feature = "edge")]
pub use edge::{handle_edge_bootstrap, handle_edge_optimize};
pub use exec::{handle_exec, handle_execute_file, handle_explain};
pub use runtime::{explain_query_str, handle_connect};
pub use version::handle_version;
