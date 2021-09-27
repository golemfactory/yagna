#[cfg(not(feature = "hybrid"))]
pub use central::*;
#[cfg(feature = "hybrid")]
pub use hybrid::*;

pub use ya_core_model::net::{
    from, NetApiError, NetDst, NetSrc, RemoteEndpoint, TryRemoteEndpoint,
};

#[cfg(any(feature = "service", test))]
mod bcast;
#[cfg(not(feature = "hybrid"))]
mod central;
#[cfg(feature = "hybrid")]
mod hybrid;
#[cfg(any(feature = "service", test))]
mod service;
