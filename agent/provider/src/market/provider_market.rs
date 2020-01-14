use super::negotiator::{Negotiator,};
use super::mock_negotiator::{AcceptAllNegotiator};
use crate::node_info::{NodeInfo};

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

    pub async fn create_offers(&mut self, node_info: &NodeInfo) -> Result<()> {
        info!("Creating initial offer.");

        let offer = self.negotiator.create_offer(node_info)?;

        info!("Subscribing to events.");

        let subscription_id = self.api.provider().subscribe(&offer).await?;
        self.offers.push(OfferSubscription{subscription_id, offer});
        Ok(())
    }

    pub async fn onshutdown(&mut self) -> Result<()>{
        info!("Unsubscribing events.");

        for offer in self.offers.iter() {
            self.api.provider().unsubscribe(&offer.subscription_id).await?;
        }
        Ok(())
    }

    pub async fn run_step(&self) -> Result<()> {
        let events = self.query_events().await?;
        self.dispatch_events(&events);

        Ok(())
    }

    async fn query_events(&self) -> Result<Vec<ProviderEvent>> {
        if self.offers.len() > 0 {

            /// Ignore all other offer. This example assumes, that there's only one.
            let provider_subscription_id = &self.offers[0].subscription_id;
            self.api.provider()
                .collect(provider_subscription_id, Some(1), Some(2))
                .await?;
        };
        Ok(vec![])
    }

    fn dispatch_events(&self, events: &Vec<ProviderEvent>) {
        for event in events.iter() {
            if let Err(error) =  self.dispatch_event(event) {
                error!("Error processing event: {}", error);
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
