const PUBLIC_PREFIX: &str = "/public/";

#[cfg(feature = "service")]
mod service;
#[cfg(feature = "service")]
pub use service::*;

mod api;
pub use api::*;
