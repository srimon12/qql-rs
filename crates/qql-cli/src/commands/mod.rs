//! CLI command handlers.

mod check;
mod convert;
mod doctor;
mod dump_migrate;
mod edge;
mod lint;
mod run;
mod runtime;
mod setup;
mod version;

#[cfg(test)]
pub(crate) use convert::source_is_canonical;
pub use convert::{handle_convert, handle_fmt};
pub use doctor::handle_doctor;
pub use dump_migrate::{handle_dump, handle_migrate};
pub use edge::handle_configure_edge;
#[cfg(feature = "edge")]
pub use edge::{handle_edge_bootstrap, handle_edge_optimize};
pub use lint::handle_lint;
#[allow(unused_imports)]
pub use run::{handle_explain, handle_run, handle_run_file, handle_run_smart};
pub use runtime::{explain_query_str, handle_connect};
pub use setup::{
    SetupOptions, handle_config_get, handle_config_path, handle_config_set, handle_config_show,
    handle_setup,
};
pub use version::handle_version;
