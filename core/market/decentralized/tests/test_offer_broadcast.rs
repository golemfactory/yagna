#[macro_use]
mod utils;

#[cfg(test)]
mod tests {
    use futures::{channel::mpsc, prelude::*};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use ya_market_decentralized::protocol::{Discovery, OfferReceived, Propagate, Reason};
    use ya_market_decentralized::SubscriptionId;

    use crate::utils::mock_node::default::*;
    use crate::utils::{wait_for_bcast, MarketServiceExt, MarketsNetwork};

    use ya_market_decentralized::testing::mock_offer::{client, sample_offer};
    use ya_market_decentralized::testing::OfferError;

    /// Test adds offer. It should be broadcasted to other nodes in the network.
    /// Than sending unsubscribe should remove Offer from other nodes.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_offer() -> Result<(), anyhow::Error> {
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_broadcast_offer")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?
            .add_market_instance("Node-3")
            .await?;

        // Add Offer on Node-1. It should be propagated to remaining nodes.
        let market1 = network.get_market("Node-1");
        let id1 = network.get_default_id("Node-1");

        let subscription_id = market1
            .subscribe_offer(&client::sample_offer(), &id1)
            .await?;
        let offer = market1.get_offer(&subscription_id).await?;

        // Expect, that Offer will appear on other nodes.
        let market2 = network.get_market("Node-2");
        let market3 = network.get_market("Node-3");
        wait_for_bcast(1000, &market2, &subscription_id, true).await;
        assert_eq!(offer, market2.get_offer(&subscription_id).await?);
        assert_eq!(offer, market3.get_offer(&subscription_id).await?);

        // Unsubscribe Offer. Wait some delay for propagation.
        market1.unsubscribe_offer(&subscription_id, &id1).await?;
        let expected_error = OfferError::AlreadyUnsubscribed(subscription_id.clone());
        assert_err_eq!(expected_error, market1.get_offer(&subscription_id).await);
        // Expect, that Offer will disappear on other nodes.
        wait_for_bcast(1000, &market2, &subscription_id, false).await;
        assert_err_eq!(expected_error, market2.get_offer(&subscription_id).await);
        assert_err_eq!(expected_error, market2.get_offer(&subscription_id).await);

        Ok(())
    }

    /// Offer subscription id should be validated on reception. If Offer
    /// id hash doesn't match hash computed from Offer fields, Market should
    /// reject such an Offer since it could be some kind of attack.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_offer_validation() -> Result<(), anyhow::Error> {
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_broadcast_offer_validation")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_discovery_instance(
                "Node-2",
                empty_on_offer_received,
                empty_on_offer_unsubscribed,
                empty_on_retrieve_offers,
            )
            .await?;

        let market1 = network.get_market("Node-1");
        let discovery2: Discovery = network.get_discovery("Node-2");

        // Prepare Offer with subscription id changed to invalid.
        let invalid_id = SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53")?;
        let mut offer = sample_offer();
        offer.id = invalid_id.clone();

        // Offer should be propagated to market1, but he should reject it.
        discovery2.broadcast_offer(offer).await?;
        tokio::time::delay_for(Duration::from_millis(50)).await;

        assert_err_eq!(
            OfferError::NotFound(invalid_id.clone()),
            market1.get_offer(&invalid_id).await,
        );
        Ok(())
    }

    /// Nodes shouldn't broadcast unsubscribed Offers.
    /// This test broadcasts unsubscribed Offer and checks how other market Nodes
    /// behave. We expect that market nodes will stop broadcast and Discovery interface will
    /// get Offer only from himself.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_stop_conditions() -> Result<(), anyhow::Error> {
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_broadcast_stop_conditions")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        // Add Offer on Node-1. It should be propagated to remaining nodes.
        let market1 = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let subscription_id = market1
            .subscribe_offer(&client::sample_offer(), &identity1)
            .await?;
        let offer = market1.get_offer(&subscription_id).await?;

        // Expect, that Offer will appear on other nodes.
        let market2 = network.get_market("Node-2");
        wait_for_bcast(1000, &market2, &subscription_id, true).await;
        assert_eq!(offer, market2.get_offer(&subscription_id).await?);

        // Unsubscribe Offer. It should be unsubscribed on all Nodes and removed from
        // database on Node-2, since it's foreign Offer.
        market1
            .unsubscribe_offer(&subscription_id, &identity1)
            .await?;
        assert_err_eq!(
            OfferError::AlreadyUnsubscribed(subscription_id.clone()),
            market1.get_offer(&subscription_id).await
        );

        // Expect, that Offer will disappear on other nodes.
        wait_for_bcast(1000, &market2, &subscription_id, false).await;
        assert_err_eq!(
            OfferError::AlreadyUnsubscribed(subscription_id.clone()),
            market2.get_offer(&subscription_id).await
        );

        // Send the same Offer using Discovery interface directly.
        // Number of returning Offers will be counted.
        let (tx, mut rx) = mpsc::channel::<()>(1);
        let offers_counter = Arc::new(AtomicUsize::new(0));
        let counter = offers_counter.clone();
        let network = network
            .add_discovery_instance(
                "Node-3",
                move |_caller: String, _msg: OfferReceived| {
                    let offers_counter = counter.clone();
                    let mut tx = tx.clone();
                    async move {
                        offers_counter.fetch_add(1, Ordering::SeqCst);
                        tx.send(()).await.unwrap();
                        Ok(Propagate::No(Reason::AlreadyExists))
                    }
                },
                empty_on_offer_unsubscribed,
                empty_on_retrieve_offers,
            )
            .await?;

        // Broadcast already unsubscribed Offer. We will count number of Offers that will come back.
        let discovery3: Discovery = network.get_discovery("Node-3");
        discovery3.broadcast_offer(offer).await?;

        // Wait for broadcast.
        rx.next().await;

        assert_eq!(
            offers_counter.load(Ordering::SeqCst),
            1,
            "We expect to receive Offer only from ourselves"
        );

        // We expect, that Offers won't be available on other nodes now
        assert_err_eq!(
            OfferError::AlreadyUnsubscribed(subscription_id.clone()),
            market1.get_offer(&subscription_id).await,
        );
        assert_err_eq!(
            OfferError::AlreadyUnsubscribed(subscription_id.clone()),
            market2.get_offer(&subscription_id).await,
        );

        Ok(())
    }
}
