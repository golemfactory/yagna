pub mod deploy;

#[cfg(feature = "server")]
pub mod server;

pub const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION");
