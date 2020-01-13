use super::negotiator::{Negotiator,};
use super::mock_negotiator::{AcceptAllNegotiator};
use crate::node_info::{NodeInfo, CpuInfo};

use ya_client::{market::{ApiClient, ProviderApi}, Result, Error};
use ya_model::market::{ProviderEvent, Offer};

use futures::executor::block_on;
use log::{info, warn, error};


struct OfferSubscription {
    subscription_id: String,
    offer: Offer,
}

pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    api: ApiClient,
    offers: Vec<OfferSubscription>,
}


impl ProviderMarket {

    pub fn new(api: ApiClient, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket{api, negotiator, offers: vec![]};
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Creating initial offer.");

        let cpu = CpuInfo{ architecture: "wasm32".to_string(), cores: 1, threads: 1 };
        let node_info = NodeInfo{ cpu: cpu, id: "Provider Node".to_string() };

        let offer = self.negotiator.create_offer(&node_info)?;

        info!("Subscribing to events.");

        let subscription_id = self.api.provider().subscribe(&offer).await?;
        self.offers.push(OfferSubscription{subscription_id, offer});
        Ok(())
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
        for event in events.iter() {
            match self.dispatch_event(event) {
                Err(error) => error!("Error processing event: {}", error),
                _ => {}
            }
        }
    }

    fn dispatch_event(&self, event: &ProviderEvent) -> Result<()> {
        unimplemented!()
    }

    fn process_proposal(&self) {
        unimplemented!()
    }

    fn process_agreement(&self) {
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
