pub mod expiration;
pub mod max_agreements;

pub use expiration::LimitExpiration;
pub use max_agreements::MaxAgreements;

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
}
