//! byte — claude account switcher

pub mod error;
pub mod output;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
