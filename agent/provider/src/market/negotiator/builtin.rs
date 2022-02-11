pub mod expiration;
pub mod max_agreements;
pub mod note_interval;
pub mod payment_timeout;

pub use expiration::LimitExpiration;
pub use max_agreements::MaxAgreements;
pub use note_interval::DebitNoteInterval;
pub use payment_timeout::PaymentTimeout;

use ya_negotiators::component::register_negotiator;
use ya_negotiators::NegotiatorComponent;

pub fn register_negotiators() {
    register_negotiator(
        "ya-provider",
        "LimitExpiration",
        Box::new(|config, _| {
            Ok(Box::new(LimitExpiration::new(config)?) as Box<dyn NegotiatorComponent>)
        }),
    );
    register_negotiator(
        "ya-provider",
        "LimitAgreements",
        Box::new(|config, _| {
            Ok(Box::new(MaxAgreements::new(config)?) as Box<dyn NegotiatorComponent>)
        }),
    );
    register_negotiator(
        "ya-provider",
        "PaymentTimeout",
        Box::new(|config, _| {
            Ok(Box::new(PaymentTimeout::new(config)?) as Box<dyn NegotiatorComponent>)
        }),
    );
    register_negotiator(
        "ya-provider",
        "DebitNoteInterval",
        Box::new(|config, _| {
            Ok(Box::new(DebitNoteInterval::new(config)?) as Box<dyn NegotiatorComponent>)
        }),
    );
}
