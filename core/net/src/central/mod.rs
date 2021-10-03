mod api;
#[cfg(any(feature = "service", test))]
mod handler;
#[cfg(any(feature = "service", test))]
mod service;

pub use api::*;
#[cfg(any(feature = "service", test))]
pub use service::{bind_remote, Net};

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use ya_core_model::net::local::Subscribe;

lazy_static::lazy_static! {
    pub(crate) static ref SUBSCRIPTIONS: Arc<Mutex<HashSet<Subscribe>>> = Default::default();
}
