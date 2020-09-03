//! Cyclic methods for Matcher spawned after binding to GSB
use rand::seq::IteratorRandom;
use rand::Rng;
use std::collections::HashSet;

use super::Matcher;
use crate::db::model::SubscriptionId;

pub(super) async fn bcast_offers(matcher: Matcher) {
    if matcher.config.discovery.max_bcasted_offers <= 0 {
        return;
    }

    let bcast_interval = matcher.config.discovery.mean_cyclic_bcast_interval;
    loop {
        let matcher = matcher.clone();
        async move {
            wait_random_interval(bcast_interval).await;

            // We always broadcast our own Offers.
            let our_ids = matcher.get_our_active_offer_ids().await?;

            // Add some random subset of Offers to broadcast.
            // TODO: We will send more Offers, than config states, if we have many own Offers.
            let num_our_offers = our_ids.len();
            let num_to_bcast = matcher.config.discovery.max_bcasted_offers;

            let all_ids = matcher.store.get_active_offer_ids(None).await?;
            let our_and_random_ids = randomize_ids(our_ids, all_ids, num_to_bcast as usize);

            log::debug!(
                "Cyclic bcast: Sending {} Offers including {} ours.",
                our_and_random_ids.len(),
                num_our_offers
            );

            matcher.discovery.bcast_offers(our_and_random_ids).await?;
            Result::<(), anyhow::Error>::Ok(())
        }
        .await
        .map_err(|e| log::warn!("Failed to send random subscriptions bcast. Error: {}", e))
        .ok();
    }
}

pub(super) async fn bcast_unsubscribes(matcher: Matcher) {
    if matcher.config.discovery.max_bcasted_unsubscribes <= 0 {
        return;
    }

    let bcast_interval = matcher.config.discovery.mean_cyclic_unsubscribes_interval;
    loop {
        let matcher = matcher.clone();
        async move {
            wait_random_interval(bcast_interval).await;

            // We always broadcast our own Offer unsubscribes.
            let our_ids = matcher.get_our_unsubscribed_offer_ids().await?;

            // Add some random subset of Offer unsubscribes to bcast.
            // TODO: We will send more unsubscribes, than config states, if we have many own unsubscribes.
            let num_our_unsubscribes = our_ids.len();
            let max_bcast = matcher.config.discovery.max_bcasted_unsubscribes as usize;

            let all_ids = matcher.store.get_unsubscribed_offer_ids(None).await?;
            let our_and_random_ids = randomize_ids(our_ids, all_ids, max_bcast);

            log::debug!(
                "Cyclic bcast: Sending {} unsubscribes including {} ours.",
                our_and_random_ids.len(),
                num_our_unsubscribes
            );

            matcher
                .discovery
                .bcast_unsubscribes(our_and_random_ids)
                .await?;
            Result::<(), anyhow::Error>::Ok(())
        }
        .await
        .map_err(|e| log::warn!("Failed to send random unsubscribes bcast. Error: {}", e))
        .ok();
    }
}

/// Returns vector of at most `cap_size` getting all our ids
/// and random sample from other ids (all ids might include our ids).
fn randomize_ids(
    our_ids: Vec<SubscriptionId>,
    all_ids: Vec<SubscriptionId>,
    cap_size: usize,
) -> Vec<SubscriptionId> {
    let our_len = our_ids.len();
    if our_len > cap_size {
        log::warn!("Our ids count: {} exceed cap: {}", our_len, cap_size);
        return our_ids;
    }
    // Filter our Offers from set.
    let num_to_select = (cap_size - our_len).max(0);
    let our_ids = our_ids.into_iter().collect();
    let mut randomized_ids = all_ids
        .into_iter()
        .collect::<HashSet<SubscriptionId>>()
        .difference(&our_ids)
        .cloned()
        .choose_multiple(&mut rand::thread_rng(), num_to_select);
    randomized_ids.extend(our_ids);
    randomized_ids
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

        let offers = randomize_ids(our_offers, all_offers, 2);

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

        let offers = randomize_ids(our_offers, all_offers, 4);

        // All Offers should be included.
        assert!(offers.contains(&sub1));
        assert!(offers.contains(&sub2));
        assert!(offers.contains(&sub3));
        assert_eq!(offers.len(), 3);
    }
}
