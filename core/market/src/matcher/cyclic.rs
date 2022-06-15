//! Cyclic methods for Matcher spawned after binding to GSB
use metrics::{counter, timing};
use rand::seq::IteratorRandom;
use rand::Rng;
use std::collections::HashSet;
use std::hash::Hash;

use super::Matcher;
use std::time::Instant;

pub(super) async fn bcast_offers(matcher: Matcher) {
    if matcher.config.discovery.max_bcasted_offers <= 0 {
        return;
    }

    let bcast_interval = matcher.config.discovery.mean_cyclic_bcast_interval;
    loop {
        let matcher = matcher.clone();
        async move {
            wait_random_interval(bcast_interval).await;

            let start = Instant::now();

            // We always broadcast our own Offers.
            let our_ids = matcher.get_our_active_offer_ids().await?;

            // Add some random subset of Offers to broadcast.
            let num_our_offers = our_ids.len();
            let num_to_bcast = matcher.config.discovery.max_bcasted_offers;

            let all_ids = matcher.store.get_active_offer_ids(None).await?;
            let our_and_random_ids = randomize_ids(our_ids, all_ids, num_to_bcast as usize);

            log::trace!(
                "Broadcasted {} Offers including {} ours.",
                our_and_random_ids.len(),
                num_our_offers
            );

            matcher.discovery.bcast_offers(our_and_random_ids).await?;

            let end = Instant::now();
            counter!("market.offers.broadcasts", 1);
            timing!("market.offers.broadcasts.time", start, end);

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

            let start = Instant::now();

            // We always broadcast our own Offer unsubscribes.
            let our_ids = matcher.get_our_unsubscribed_offer_ids().await?;

            // Add some random subset of Offer unsubscribes to bcast.
            let num_our_unsubscribes = our_ids.len();
            let max_bcast = matcher.config.discovery.max_bcasted_unsubscribes as usize;

            let all_ids = matcher.store.get_unsubscribed_offer_ids(None).await?;
            let our_and_random_ids = randomize_ids(our_ids, all_ids, max_bcast);

            log::trace!(
                "Broadcasted {} unsubscribed Offers including {} ours.",
                our_and_random_ids.len(),
                num_our_unsubscribes
            );

            matcher
                .discovery
                .bcast_unsubscribes(our_and_random_ids)
                .await?;

            let end = Instant::now();
            counter!("market.offers.unsubscribes.broadcasts", 1);
            timing!("market.offers.unsubscribes.broadcasts.time", start, end);

            Result::<(), anyhow::Error>::Ok(())
        }
        .await
        .map_err(|e| log::warn!("Failed to send random unsubscribes bcast. Error: {}", e))
        .ok();
    }
}

/// Returns vector of at most `cap_size` getting all our ids
/// and random sample from other ids (all ids might include our ids).
#[allow(dead_code)]
fn randomize_ids<T: Eq + Hash + Clone>(
    our_ids: Vec<T>,
    all_ids: Vec<T>,
    cap_size: usize,
) -> Vec<T> {
    let our_len = our_ids.len();
    if our_len > cap_size {
        log::warn!("Our ids count: {} exceed cap: {}", our_len, cap_size);
        return our_ids
            .into_iter()
            .choose_multiple(&mut rand::thread_rng(), cap_size);
    }

    let num_to_select = (cap_size - our_len).max(0);
    let our_ids = our_ids.into_iter().collect();
    let mut randomized_ids = all_ids
        .into_iter()
        .collect::<HashSet<T>>()
        .difference(&our_ids)
        .cloned()
        .choose_multiple(&mut rand::thread_rng(), num_to_select);
    randomized_ids.extend(our_ids);
    randomized_ids
}

fn randomize_interval(mean_interval: std::time::Duration) -> std::time::Duration {
    let mut rng = rand::thread_rng();
    // randomize interval between 0.5 and 1.5 times the mean_interval
    mean_interval.mul_f64(0.5f64 + rng.gen::<f64>())
}

async fn wait_random_interval(mean_interval: std::time::Duration) {
    let random_interval = randomize_interval(mean_interval);
    tokio::time::sleep(random_interval).await;
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::model::SubscriptionId;
    use std::str::FromStr;

    #[test]
    fn test_randomize_cap_0() {
        let offers = randomize_ids(vec![1], vec![], 0);
        assert_eq!(offers.len(), 0)
    }

    #[test]
    fn test_randomize_cap_1() {
        let offers = randomize_ids(vec![7], vec![8, 9], 1);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0], 7)
    }

    #[test]
    fn test_randomize_cap_1_empty_ours() {
        let offers = randomize_ids(vec![], vec![12], 1);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0], 12)
    }

    #[test]
    fn test_randomize_cap_2_not_enough() {
        let offers = randomize_ids(vec![17], vec![], 2);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0], 17)
    }

    #[test]
    fn test_randomize_cap_2() {
        let offers = randomize_ids(vec![1], vec![1, 2, 3], 2);

        // Our Offer must be included.
        assert!(offers.contains(&1));
        // One of someone's else Offer must be included.
        assert!(offers.contains(&2) | offers.contains(&3));
        assert_eq!(offers.len(), 2);
    }

    #[test]
    fn test_randomize_offers_cap_4() {
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
