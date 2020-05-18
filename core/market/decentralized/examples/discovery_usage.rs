use anyhow;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tokio::runtime::Runtime;

use ya_client::model::market::Offer;
use ya_market_decentralized::protocol::callbacks::HandlerSlot;
use ya_market_decentralized::protocol::{
    Discovery, DiscoveryBuilder, DiscoveryError, DiscoveryFactory, DiscoveryInitError,
    OfferReceived, RetrieveOffers,
};

// =========================================== //
// TODO: Remove this example after implementing Discovery
// =========================================== //

/// Example implementation of Discovery.
struct DiscoveryExample {
    offer_received: HandlerSlot<OfferReceived>,
    retrieve_offers: HandlerSlot<RetrieveOffers>,
}

impl DiscoveryFactory for DiscoveryExample {
    fn new(mut builder: DiscoveryBuilder) -> Result<Arc<dyn Discovery>, DiscoveryInitError> {
        let offer_received = builder.offer_received_handler()?;
        let retrieve_offers = builder.retrieve_offers_handler()?;

        let discovery = DiscoveryExample {
            offer_received,
            retrieve_offers,
        };
        Ok(Arc::new(discovery))
    }
}

#[async_trait]
impl Discovery for DiscoveryExample {
    async fn bind_gsb(&self, _prefix: String) -> Result<(), DiscoveryInitError> {
        Ok(())
    }

    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError> {
        Ok(self
            .offer_received
            .call(format!("caller"), OfferReceived { offer })
            .await?)
    }

    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError> {
        Ok(self
            .retrieve_offers
            .call(
                format!("caller"),
                RetrieveOffers {
                    newer_than: Utc::now(),
                },
            )
            .await?)
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let builder = DiscoveryBuilder::new()
        .bind_offer_received(move |msg: OfferReceived| async move {
            log::info!("Offer from [{}] received.", msg.offer.offer_id.unwrap());
            Ok(())
        })
        .bind_retrieve_offers(move |_msg: RetrieveOffers| async move {
            log::info!("Offers request received.");
            Ok(vec![])
        });
    let discovery = DiscoveryExample::new(builder)?;
    let dicovery_clone = discovery.clone();

    std::thread::spawn(move || {
        let offer = mock_offer(format!("Caller-thread"));

        let mut rt = Runtime::new().unwrap();
        rt.block_on(dicovery_clone.broadcast_offer(offer))
    });

    let offer = mock_offer(format!("Caller-local"));
    discovery.broadcast_offer(offer).await?;

    Ok(())
}

fn mock_offer(caller: String) -> Offer {
    let mut offer = Offer::new(json!({}), format!(""));
    offer.offer_id = Some(caller);
    offer
}
