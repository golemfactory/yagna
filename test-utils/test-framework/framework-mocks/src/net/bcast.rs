use std::collections::btree_map::Entry::{Occupied, Vacant};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ya_client::model::NodeId;
use ya_core_model::net::local as local_net;
use ya_framework_basic::mocks::net::IMockBroadcast;

#[derive(Clone)]
pub struct BCastService {
    inner: Arc<Mutex<BCastServiceInner>>,
}

#[derive(Default)]
struct BCastServiceInner {
    topics_endpoints: BTreeMap<String, Vec<Arc<str>>>,
    node_subnet: BTreeMap<String, String>,
}

impl Default for BCastService {
    fn default() -> Self {
        BCastService::new()
    }
}

impl BCastService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl IMockBroadcast for BCastService {
    fn register_for_broadcasts(&self, node_id: &NodeId, subnet: &str) {
        let mut me = self.inner.lock().unwrap();
        log::info!("registering node {} within subnet: {}", node_id, subnet);

        match me.node_subnet.entry(node_id.to_string()) {
            Occupied(_) => panic!(
                "node {} already registered in BCast subnet: {}",
                node_id, subnet
            ),
            Vacant(entry) => entry.insert(subnet.to_string()),
        };
    }

    fn subscribe_topic(&self, subscribe: local_net::Subscribe) {
        let mut me = self.inner.lock().unwrap();
        me.topics_endpoints
            .entry(subscribe.topic().to_owned())
            .or_default()
            .push(subscribe.endpoint().into());
    }

    fn resolve(&self, node_id: &str, topic: &str) -> Vec<Arc<str>> {
        let me = self.inner.lock().unwrap();
        let subnet = match me.node_subnet.get(node_id) {
            Some(subnet) => format!("/{}/", subnet),
            None => panic!("node {} is not registered for BCast", node_id),
        };
        me.topics_endpoints
            .get(topic)
            .map(|receivers| {
                receivers
                    .iter()
                    .filter(|endpoint| endpoint.starts_with(&subnet))
                    .map(Clone::clone)
                    .collect()
            })
            .unwrap_or_default()
    }
}
