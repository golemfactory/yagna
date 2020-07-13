// TODO: move to ../<mod_name>.rs
// Could be private
pub mod callbacks; // TODO: remove plural form
mod discovery;
pub mod negotiation;

pub use self::discovery::{builder::DiscoveryBuilder, Discovery, Propagate, Reason};
pub use self::discovery::{DiscoveryError, DiscoveryInitError, DiscoveryRemoteError};
pub use self::discovery::{OfferReceived, OfferUnsubscribed, RetrieveOffers};

pub use self::callbacks::{CallbackHandler, CallbackMessage, HandlerSlot};
