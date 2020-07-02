// Could be private
pub mod callbacks;
mod discovery;
pub mod negotiation;

pub use self::discovery::{Discovery, Propagate, Reason};
pub use self::discovery::{DiscoveryError, DiscoveryInitError, DiscoveryRemoteError};
pub use self::discovery::{OfferReceived, OfferUnsubscribed, RetrieveOffers};

pub use self::callbacks::{CallbackHandler, CallbackMessage, HandlerSlot};
