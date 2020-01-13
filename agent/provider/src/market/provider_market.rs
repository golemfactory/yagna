use super::negotiator::{Negotiator,};
use super::mock_negotiator::{AcceptAllNegotiator};
use log::{warn};

use ya_client::{market::{ApiClient, ProviderApi}, Result, Error};
use ya_model::market::{ProviderEvent};



pub struct ProviderMarket {
    pub negotiator: Box<dyn Negotiator>,
    api: ApiClient,
}


impl ProviderMarket {

    pub fn new(api: ApiClient, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket{api, negotiator};
    }

    pub fn run_step(&self) -> Result<()> {
        let events = self.query_events()?;
        self.dispatch_events(&events);

        Ok(())
    }

    fn query_events(&self) -> Result<Vec<ProviderEvent>> {
        unimplemented!()
    }

    fn dispatch_events(&self, events: &Vec<ProviderEvent>) {
        unimplemented!()
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
