//! Broadcast singleton support service
// Singleton version of BCast module in hope to solve problems with running
// multithreaded integration tests with singleton GSB router

use std::collections::btree_map::Entry::{Occupied, Vacant};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::BCast;
use ya_client::model::NodeId;
use ya_core_model::net::local as local_net;

#[derive(Clone)]
pub struct BCastService {
    inner: Arc<Mutex<BCastServiceInner>>,
}

#[derive(Default)]
struct BCastServiceInner {
    topics_endpoints: BTreeMap<String, Vec<Arc<str>>>,
    node_subnet: BTreeMap<String, String>,
}

lazy_static::lazy_static! {
    static ref BCAST : BCastService = BCastService {
        inner: Arc::new(Mutex::new(BCastServiceInner::default()))
    };
}

impl Default for BCastService {
    fn default() -> Self {
        log::debug!("getting singleton mock BCast");
        (*BCAST).clone()
    }
}

impl BCast for BCastService {
    fn register(&self, node_id: &NodeId, subnet: &str) {
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

    fn add(&self, subscribe: local_net::Subscribe) {
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
