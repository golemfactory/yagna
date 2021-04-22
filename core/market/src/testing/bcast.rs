//! Broadcast support service
// Note: This file is derived from core/net module. It serves only as mock
// so we don't have to keep it compatible.
// It was moved here, because this file is not expected to be public in net module.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use ya_client::model::NodeId;
use ya_core_model::net;

use super::{MarketServiceExt, QueryOfferError, SubscriptionId};
use crate::MarketService;

pub mod singleton;

pub trait BCast: Clone {
    /// registers node to be visible only in specific subnet
    fn register(&self, _node_id: &NodeId, _subnet: &str) {}
    /// subscribes endpoint to topic; endpoint prefix is /<subnet>/
    fn add(&self, subscribe: net::local::Subscribe);
    /// returns all nodes with same subnet as given node subscribed to given topic
    fn resolve(&self, node_id: &str, topic: &str) -> Vec<Arc<str>>;
}

#[derive(Clone, Default)]
pub struct BCastService {
    inner: Arc<RefCell<BCastServiceInner>>,
}

#[derive(Default)]
struct BCastServiceInner {
    topics: BTreeMap<String, Vec<Arc<str>>>,
}

impl BCast for BCastService {
    fn add(&self, subscribe: net::local::Subscribe) {
        let mut me = self.inner.borrow_mut();
        me.topics
            .entry(subscribe.topic().to_owned())
            .or_insert_with(Default::default)
            .push(subscribe.endpoint().into())
    }

    fn resolve(&self, _node_id: &str, topic: &str) -> Vec<Arc<str>> {
        let me = self.inner.borrow();
        me.topics
            .get(topic)
            .map(|receivers| receivers.iter().map(|endpoint| endpoint.clone()).collect())
            .unwrap_or_default()
    }
}

/// Assure that all given nodes have the same knowledge about given Subscriptions (Offers).
/// Wait if needed at most 1,5s ( = 10 x 150ms).
pub async fn assert_offers_broadcasted<'a, S>(mkts: &[&MarketService], subscriptions: S)
where
    S: IntoIterator<Item = &'a SubscriptionId>,
    <S as IntoIterator>::IntoIter: Clone,
{
    let subscriptions = subscriptions.into_iter();
    let mut all_broadcasted = false;
    'retry: for _i in 0..10 {
        for subscription in subscriptions.clone() {
            for mkt in mkts {
                if mkt.get_offer(&subscription).await.is_err() {
                    // Every 150ms we should get at least one broadcast from each Node.
                    // After a few tries all nodes should have the same knowledge about Offers.
                    tokio::time::delay_for(Duration::from_millis(150)).await;
                    continue 'retry;
                }
            }
        }
        all_broadcasted = true;
        break;
    }
    assert!(
        all_broadcasted,
        "At least one of the offers was not propagated to all nodes"
    );
}

/// Assure that all given nodes have the same knowledge about given Subscriptions (Offers).
/// Wait if needed at most 1,5s ( = 10 x 150ms).
pub async fn assert_unsunbscribes_broadcasted<'a, S>(mkts: &[&MarketService], subscriptions: S)
where
    S: IntoIterator<Item = &'a SubscriptionId>,
    <S as IntoIterator>::IntoIter: Clone,
{
    let subscriptions = subscriptions.into_iter();
    let mut all_broadcasted = false;
    'retry: for _i in 0..10 {
        for subscription in subscriptions.clone() {
            for mkt in mkts {
                let expect_error = QueryOfferError::Unsubscribed(subscription.clone()).to_string();
                match mkt.get_offer(&subscription).await {
                    Err(e) => assert_eq!(e.to_string(), expect_error),
                    Ok(_) => {
                        // Every 150ms we should get at least one broadcast from each Node.
                        // After a few tries all nodes should have the same knowledge about Offers.
                        tokio::time::delay_for(Duration::from_millis(150)).await;
                        continue 'retry;
                    }
                }
            }
        }
        all_broadcasted = true;
        break;
    }
    assert!(
        all_broadcasted,
        "At least one of the offer unsubscribes was not propagated to all nodes"
    );
}
