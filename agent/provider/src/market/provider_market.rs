use super::negotiator::{Negotiator,};
use super::mock_negotiator::{AcceptAllNegotiator};
use log::{warn};

use ya_client::{market::{ApiClient, ProviderApi},};



pub struct ProviderMarket {
    pub negotiator: Box<dyn Negotiator>,
    api: ApiClient,
}


impl ProviderMarket {

    pub fn new(api: ApiClient, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket{api, negotiator};
    }

    pub fn run() {

    }
}


fn create_negotiator(name: &str) -> Box<dyn Negotiator> {
    match name {
        "AcceptAll" => Box::new(AcceptAllNegotiator::new()),
        _ => {
            warn!("Unknown negotiator type {}. Using default: AcceptAll", name);
            Box::new(AcceptAllNegotiator::new())
        }
    }
}
