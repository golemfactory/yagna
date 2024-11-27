mod api;
pub(crate) mod cli;
mod codec;
mod crypto;
mod rest_api;
mod service;

pub use api::*;
pub use rest_api::web_scope;
pub use service::{start_network, Net};

pub mod testing {
    pub use crate::iroh::service::{parse_from_to_addr, parse_net_to_addr};
}
