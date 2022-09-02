//! Command line handling
pub mod clean;
pub mod config;
pub mod exe_unit;
pub mod keystore;
pub mod preset;
pub mod profile;
pub mod whitelist;

use crate::startup_config::ProviderConfig;

/// Prints line if `json` output disabled.
fn println_conditional(config: &ProviderConfig, txt: &str) {
    if !config.json {
        println!("{txt}");
    }
}
