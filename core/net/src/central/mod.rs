mod api;
mod handler;
mod service;

pub use api::*;
pub use service::{bind_remote, Net};

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use ya_core_model::net::local::Subscribe;

lazy_static::lazy_static! {
    pub(crate) static ref SUBSCRIPTIONS: Arc<Mutex<HashSet<Subscribe>>> = Default::default();
}
