//! Parameter binding and prepared query substitution.
//!
//! Provides type-safe substitution of named (`:name`) and positional (`?`)
//! parameter placeholders in QQL query text.
//!
//! ### Placeholders & Syntax Rules
//!
//! - **Named placeholders**: `:name` (e.g. `:category`, `:limit`).
//! - **Positional placeholders**: `?` (sequential 1-to-1 mapping with parameters list).
//!
//! In QQL, `$` is a first-class identifier character (e.g. `$category`, `$1`), so
//! parameter placeholders exclusively use `:name` and `?`. This guarantees that
//! `$`-prefixed identifiers in queries are never accidentally or silently rewritten.
//!
//! Furthermore, a colon `:` is only recognized as a parameter placeholder when it occurs
//! at a valid token boundary (preceded by whitespace, punctuation, or start of query).
//! Colons in compact dictionary syntax (e.g. `{a:b}`, `{'a':b}`) are not placeholders
//! and are preserved without modification. Note that unconventional spacing with whitespace
//! before the colon (`{a :b}`) makes `:b` lexically indistinguishable from a placeholder.
//!
//! Literals and dictionary keys are safely formatted and escaped to prevent
//! query injection breakouts. String literals (`'...'`, `r'...'`, `"""..."""`,
//! and `` `...` ``) and comments (`-- ...`) in the source query are preserved
//! verbatim and never substituted.

pub(crate) mod ast;
pub(crate) mod text;
pub(crate) mod validate;

#[cfg(test)]
mod tests;

pub use ast::{
    bind_filter, bind_formula, bind_page_spec, bind_point_id, bind_query_expr, bind_query_input,
    bind_query_stmt, bind_stmt, bind_value,
};
pub use text::{
    bind_named, bind_named_readable, bind_positional, bind_positional_readable,
    truncate_vector_literals, value_to_literal,
};
pub use validate::validate_no_unbound_params;
