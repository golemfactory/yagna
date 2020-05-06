use tokio::sync::mpsc;

use ya_client_model::market::Offer;


/// Responsible for communication with markets on other nodes
/// during discovery phase.
pub trait DiscoveryAPI {
    async fn broadcast_offer(self, offer: Offer) -> Result<()>;
    fn offers_receiver(self) -> Receiver;
}

