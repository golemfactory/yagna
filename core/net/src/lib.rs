pub use ya_core_model::net::{
    from, NetApiError, NetDst, NetSrc, RemoteEndpoint, TryRemoteEndpoint,
};

pub use service::{bind_broadcast_with_caller, broadcast, Net};

#[cfg(any(feature = "service", test))]
mod bcast;
mod central;
mod hybrid;
#[cfg(any(feature = "service", test))]
mod service;

mod config;
