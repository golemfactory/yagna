use anyhow::Result;
use std::sync::Arc;

use ya_client_model::NodeId;
use ya_core_model::net;

pub trait IMockBroadcast {
    /// Registers node to be visible only in specific subnet
    fn register_for_broadcasts(&self, _node_id: &NodeId, _subnet: &str);
    /// Subscribes endpoint to topic; endpoint prefix is /<subnet>/
    fn subscribe_topic(&self, subscribe: net::local::Subscribe);
    /// Returns all nodes with same subnet as given node subscribed to given topic
    fn resolve(&self, node_id: &str, topic: &str) -> Vec<Arc<str>>;
}

pub trait IMockNet: IMockBroadcast {
    fn bind_gsb(&self);
    fn register_node(&self, node_id: &NodeId, prefix: &str);
    fn unregister_node(&self, node_id: &NodeId) -> Result<()>;
}

pub fn gsb_prefixes(test_name: &str, name: &str) -> (String, String) {
    let public_gsb_prefix = format!("/{}/{}/public", test_name, name);
    let local_gsb_prefix = format!("/{}/{}/local", test_name, name);
    (public_gsb_prefix, local_gsb_prefix)
}

pub fn gsb_market_prefixes(public: &str, local: &str) -> (String, String) {
    let public_gsb_prefix = format!("{}/market", public);
    let local_gsb_prefix = format!("{}/market", local);
    (public_gsb_prefix, local_gsb_prefix)
}
