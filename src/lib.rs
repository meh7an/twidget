//! twidget interprets Telegram live widget documents.
//!
//! It parses envelopes and templates under budgets, evaluates expressions, and writes the
//! resolved tree that each app lays out and paints natively. `docs/SPEC.md` defines the
//! behavior, and `include/twidget.h` is the C ABI.
//!
//! Core contracts:
//! - hostile input yields an [`Error`], never a panic;
//! - no threads, clocks, callbacks or I/O: time and theme arrive with every call;
//! - the same inputs produce the same bytes on every platform.

#![deny(missing_docs)]

// The crate always links std (see the `std` feature in Cargo.toml), but library code names `core`
// and `alloc` paths so that every use of `std` stays visible: the C API and the CLI.
extern crate alloc;

pub mod budget;
mod error;
pub mod expr;
pub mod json;
pub mod model;
pub mod path;
pub mod resolve;

pub use error::{Error, Result, Status};
