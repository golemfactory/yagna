//! Broadcast support service
// Note: This file is derived from core/net module. It serves only as mock
// so we don't have to keep it compatible.
// It was moved here, because this file is not expected to be public in net module.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use ya_client::model::NodeId;
use ya_core_model::net;

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
            .map(|receivers| receivers.to_vec())
            .unwrap_or_default()
    }
}
