// Could be private
pub mod callbacks;
mod discovery;

pub use self::discovery::{Discovery, DiscoveryBuilder, PropagateOffer, StopPropagateReason};
pub use self::discovery::{DiscoveryError, DiscoveryInitError, DiscoveryRemoteError};
pub use self::discovery::{OfferReceived, RetrieveOffers};

pub use self::callbacks::{CallbackHandler, HandlerSlot};
