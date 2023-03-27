mod api;
pub(crate) mod cli;
mod handler;
mod rest_api;
mod service;

pub use api::*;
pub use rest_api::web_scope;
pub use service::{bind_remote, Net};

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use ya_core_model::net::local::Subscribe;

lazy_static::lazy_static! {
    pub(crate) static ref SUBSCRIPTIONS: Arc<Mutex<HashSet<Subscribe>>> = Default::default();
}
