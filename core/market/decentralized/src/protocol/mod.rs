// Could be private
pub mod callbacks;
mod discovery;
mod negotiation;

pub use self::discovery::{Discovery, Propagate, StopPropagateReason};
pub use self::discovery::{DiscoveryError, DiscoveryInitError, DiscoveryRemoteError};
pub use self::discovery::{OfferReceived, OfferUnsubscribed, RetrieveOffers};

pub use self::callbacks::{CallbackHandler, CallbackMessage, HandlerSlot};
