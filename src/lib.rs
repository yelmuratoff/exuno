//! Exuno engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.
//!
//! Module docs name the Bash function each file was ported from. That engine
//! last shipped in 0.37.0: `git show 0.37.0:lib/helpers/<file>.sh`.

pub mod cli;
pub mod config;
pub mod engine;
pub mod error;
pub mod output;
pub mod paths;
pub mod project;
pub mod remote;
pub mod text;
pub mod transaction;
pub mod zip;

pub use error::Error;

/// Engine version from the `VERSION` file, which `release` bumps with `Cargo.toml`.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_crate_version_carries_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), super::engine_version());
    }
}
