#[cfg(not(feature = "hybrid-net"))]
pub use central::*;
#[cfg(feature = "hybrid-net")]
pub use hybrid::*;

pub use ya_core_model::net::{
    from, NetApiError, NetDst, NetSrc, RemoteEndpoint, TryRemoteEndpoint,
};

#[cfg(any(feature = "service", test))]
mod bcast;
#[cfg(not(feature = "hybrid-net"))]
mod central;
#[cfg(feature = "hybrid-net")]
mod hybrid;
#[cfg(any(feature = "service", test))]
mod service;
