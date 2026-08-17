//! byte — claude account switcher

pub mod atomic;
pub mod error;
pub mod output;
pub mod paths;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
