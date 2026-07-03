#![no_std]
extern crate alloc;

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod token;

#[cfg(test)]
mod error_test;

#[cfg(test)]
mod lexer_test;

#[cfg(test)]
mod parser_test;

#[cfg(test)]
mod ast_test;
