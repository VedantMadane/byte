//! byte — claude account switcher

pub mod atomic;
pub mod claude;
pub mod cli;
pub mod error;
pub mod ops;
pub mod output;
pub mod paths;
pub mod store;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
