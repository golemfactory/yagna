//! Cyclic methods for Matcher spawned after binding to GSB
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashSet;
use std::iter::FromIterator;

use super::Matcher;
use crate::db::model::SubscriptionId;

pub(super) async fn bcast_offers(matcher: Matcher) {
    let bcast_interval = matcher.config.discovery.mean_cyclic_bcast_interval;
    loop {
        let matcher = matcher.clone();
        async move {
            wait_random_interval(bcast_interval).await;

            // We always broadcast our own Offers.
            let our_offers = matcher
                .list_our_offers()
                .await?
                .into_iter()
                .map(|offer| offer.id)
                .collect::<Vec<SubscriptionId>>();

            // Add some random subset of Offers to broadcast.
            let num_ours_offers = our_offers.len();
            let num_to_bcast = matcher.config.discovery.num_bcasted_offers;

            // TODO: Don't query full Offers from database if we only need ids.
            let all_offers = matcher
                .store
                .get_offers(None)
                .await?
                .into_iter()
                .map(|offer| offer.id)
                .collect::<Vec<SubscriptionId>>();
            let random_offers = randomize_offers(our_offers, all_offers, num_to_bcast as usize);

            log::debug!(
                "Cyclic bcast: Sending {} Offers including {} ours.",
                random_offers.len(),
                num_ours_offers
            );

            matcher.discovery.bcast_offers(random_offers).await?;
            Result::<(), anyhow::Error>::Ok(())
        }
        .await
        .map_err(|e| log::warn!("Failed to send random subscriptions bcast. Error: {}", e))
        .ok();
    }
}

pub(super) async fn bcast_unsubscribes(matcher: Matcher) {
    let bcast_interval = matcher.config.discovery.mean_cyclic_unsubscribes_interval;
    loop {
        let matcher = matcher.clone();
        async move {
            wait_random_interval(bcast_interval).await;

            // We always broadcast our own Offer unsubscribes.
            let our_offers = matcher.list_our_unsubscribed_offers().await?;

            // Add some random subset of Offer unsubscribes to bcast.
            let num_ours_offers = our_offers.len();
            let num_to_bcast = matcher.config.discovery.num_bcasted_unsubscribes;

            let all_offers = matcher.store.get_unsubscribed_offers(None).await?;
            let our_and_random_offers =
                randomize_offers(our_offers, all_offers, num_to_bcast as usize);

            log::debug!(
                "Cyclic bcast: Sending {} unsubscribes including {} ours.",
                our_and_random_offers.len(),
                num_ours_offers
            );

            matcher
                .discovery
                .bcast_unsubscribes(our_and_random_offers)
                .await?;
            Result::<(), anyhow::Error>::Ok(())
        }
        .await
        .map_err(|e| log::warn!("Failed to send random unsubscribes bcast. Error: {}", e))
        .ok();
    }
}

/// Chooses subset of all our Offers, that contains all of our
/// own Offers and is extended with random Offers, that came from other Nodes.
fn randomize_offers(
    our_offers: Vec<SubscriptionId>,
    all_offers: Vec<SubscriptionId>,
    max_offers: usize,
) -> Vec<SubscriptionId> {
    // Filter our Offers from set.
    let num_to_select = (max_offers - our_offers.len()).max(0);
    let all_offers_wo_ours = all_offers
        .into_iter()
        .collect::<HashSet<SubscriptionId>>()
        .difference(&HashSet::from_iter(our_offers.clone().into_iter()))
        .cloned()
        .collect::<Vec<SubscriptionId>>();
    let mut random_offers = all_offers_wo_ours
        .choose_multiple(&mut rand::thread_rng(), num_to_select)
        .cloned()
        .collect::<Vec<SubscriptionId>>();
    random_offers.extend(our_offers);
    random_offers
}

fn randomize_interval(mean_interval: std::time::Duration) -> std::time::Duration {
    let mut rng = rand::thread_rng();
    (2 * mean_interval).mul_f64(rng.gen::<f64>())
}

async fn wait_random_interval(mean_interval: std::time::Duration) {
    let random_interval = randomize_interval(mean_interval);
    tokio::time::delay_for(random_interval).await;
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_randomize_offers_max_2() {
        let base_sub_id = "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a5";
        let sub1 = SubscriptionId::from_str(&format!("{}{}", base_sub_id, 1)).unwrap();
        let sub2 = SubscriptionId::from_str(&format!("{}{}", base_sub_id, 2)).unwrap();
        let sub3 = SubscriptionId::from_str(&format!("{}{}", base_sub_id, 3)).unwrap();

        let our_offers = vec![sub1.clone()];
        let all_offers = vec![sub1.clone(), sub2.clone(), sub3.clone()];

        let offers = randomize_offers(our_offers, all_offers, 2);

        // Our Offer must be included.
        assert!(offers.contains(&sub1));
        // One of someone's else Offer must be included.
        assert!(offers.contains(&sub2) | offers.contains(&sub3));
        assert_eq!(offers.len(), 2);
    }

    #[test]
    fn test_randomize_offers_max_4() {
        let base_sub_id = "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a5";
        let sub1 = SubscriptionId::from_str(&format!("{}{}", base_sub_id, 1)).unwrap();
        let sub2 = SubscriptionId::from_str(&format!("{}{}", base_sub_id, 2)).unwrap();
        let sub3 = SubscriptionId::from_str(&format!("{}{}", base_sub_id, 3)).unwrap();

        let our_offers = vec![sub1.clone()];
        let all_offers = vec![sub1.clone(), sub2.clone(), sub3.clone()];

        let offers = randomize_offers(our_offers, all_offers, 4);

        // All Offers should be included.
        assert!(offers.contains(&sub1));
        assert!(offers.contains(&sub2));
        assert!(offers.contains(&sub3));
        assert_eq!(offers.len(), 3);
    }
}
