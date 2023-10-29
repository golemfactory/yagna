pub mod demand_validation;
pub mod expiration;
pub mod manifest;
pub mod max_agreements;
pub mod note_interval;
pub mod payment_timeout;

pub use expiration::LimitExpiration;
pub use manifest::ManifestSignature;
pub use max_agreements::MaxAgreements;
pub use note_interval::DebitNoteInterval;
pub use payment_timeout::PaymentTimeout;

use ya_negotiators::lib::{factory, register_negotiator};

pub fn register_negotiators() {
    register_negotiator(
        "ya-provider",
        "LimitExpiration",
        factory::<LimitExpiration>(),
    );
    register_negotiator("ya-provider", "LimitAgreements", factory::<MaxAgreements>());
    register_negotiator("ya-provider", "PaymentTimeout", factory::<PaymentTimeout>());
    register_negotiator(
        "ya-provider",
        "DebitNoteInterval",
        factory::<DebitNoteInterval>(),
    );
    register_negotiator(
        "ya-provider",
        "ManifestSignature",
        factory::<ManifestSignature>(),
    );
}
