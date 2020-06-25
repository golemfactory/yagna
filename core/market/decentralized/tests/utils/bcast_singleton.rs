// Broadcast support service

// Note: This file is copied from core/net module. It serves only as mock
// so we don't have to keep it compatible.
// It was moved here, because this file is not expected to be public in net module.

use std::cell::RefCell;
use std::collections::btree_map::Entry::{Occupied, Vacant};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

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
    initialized: bool,
}

lazy_static::lazy_static! {
    static ref BCAST : BCastService = BCastService {
        inner: Arc::new(Mutex::new(BCastServiceInner::default()))
    };
}

impl Default for BCastService {
    fn default() -> Self {
        (*BCAST).clone()
    }
}

impl BCastService {
    pub fn initialize(&self) {
        let mut me = self.inner.lock().unwrap();
        me.initialized = true;
    }

    pub fn is_initialized(&self) -> bool {
        let me = self.inner.lock().unwrap();
        me.initialized
    }

    pub fn register(&self, node_id: &NodeId, subnet: &str) {
        let mut me = self.inner.lock().unwrap();

        match me.node_subnet.entry(node_id.to_string()) {
            Occupied(entry) => panic!(
                "node {} already registered in BCast subnet: {}",
                node_id, subnet
            ),
            Vacant(entry) => entry.insert(subnet.to_string()),
        };
    }

    pub fn add(&self, subscribe: local_net::Subscribe) {
        let mut me = self.inner.lock().unwrap();
        me.topics_endpoints
            .entry(subscribe.topic().to_owned())
            .or_insert_with(Default::default)
            .push(subscribe.endpoint().into());
    }

    pub fn resolve(&self, node_id: &str, topic: &str) -> Vec<Arc<str>> {
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
