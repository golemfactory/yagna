//! Yagna internal definitions for service bus API.

#[cfg(feature = "activity")]
pub mod activity;

#[cfg(feature = "appkey")]
pub mod appkey;

// `payment` won't compile without `driver`
#[cfg(any(feature = "driver", feature = "payment"))]
pub mod driver;

#[cfg(feature = "identity")]
pub mod identity;

#[cfg(feature = "market")]
pub mod market;

#[cfg(feature = "net")]
pub mod net;

#[cfg(feature = "payment")]
pub mod payment;

#[cfg(feature = "gftp")]
pub mod gftp;

#[cfg(feature = "sgx")]
pub mod sgx;

use derive_more::Display;
use serde::{Deserialize, Serialize};
pub use ya_client_model::NodeId;

#[derive(Clone, Copy, Debug, Display, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Provider,
    Requestor,
}
