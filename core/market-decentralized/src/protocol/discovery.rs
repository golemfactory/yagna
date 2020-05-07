use async_trait::async_trait;
use tokio::sync::mpsc::{channel, Receiver};
use thiserror::Error;

use ya_client_model::market::Offer;

#[derive(Error, Debug)]
pub enum DiscoveryError {

}


/// Responsible for communication with markets on other nodes
/// during discovery phase.
#[async_trait]
pub trait Discovery {
    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError>;
    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError>;

    fn offers_receiver(&self) -> Receiver<Offer>;
}

