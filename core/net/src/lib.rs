#[cfg(any(feature = "service", test))]
mod bcast;
#[cfg(any(feature = "service", test))]
mod handler;
#[cfg(any(feature = "service", test))]
mod service;

#[cfg(feature = "service")]
pub use service::*;

mod api;
pub use api::*;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use ya_core_model::net::local::Subscribe;

lazy_static::lazy_static! {
    pub(crate) static ref SUBSCRIPTIONS: Arc<Mutex<HashSet<Subscribe>>> = Arc::new(Mutex::new(HashSet::new()));
}
