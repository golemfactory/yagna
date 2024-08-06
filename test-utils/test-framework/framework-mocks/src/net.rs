use anyhow::Result;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use ya_client::model::NodeId;
use ya_core_model::net;
use ya_core_model::net::local::{SendBroadcastStub, Subscribe};
use ya_core_model::net::{local as local_net, local::SendBroadcastMessage};
pub use ya_framework_basic::mocks::net::{IMockBroadcast, IMockNet};
use ya_net::hybrid::testing::{parse_from_to_addr, parse_net_to_addr};
use ya_service_bus::{serialization, typed as bus, untyped as local_bus, Error, RpcMessage};

use bcast::BCastService;

pub mod bcast;

#[derive(Clone)]
pub struct MockNet {
    inner: Arc<Mutex<MockNetInner>>,
    broadcast: BCastService,
}

#[derive(Default)]
struct MockNetInner {
    /// Maps NodeIds to gsb prefixes of other nodes.
    pub nodes: HashMap<NodeId, String>,
}

impl Default for MockNet {
    fn default() -> Self {
        MockNet::new()
    }
}

impl IMockBroadcast for MockNet {
    fn register_for_broadcasts(&self, node_id: &NodeId, subnet: &str) {
        self.broadcast.register_for_broadcasts(node_id, subnet)
    }

    fn subscribe_topic(&self, subscribe: Subscribe) {
        self.broadcast.subscribe_topic(subscribe)
    }

    fn resolve(&self, node_id: &str, topic: &str) -> Vec<Arc<str>> {
        self.broadcast.resolve(node_id, topic)
    }
}

impl IMockNet for MockNet {
    fn bind_gsb(&self) {
        self.bind_gsb_inner()
    }

    fn register_node(&self, node_id: &NodeId, prefix: &str) {
        log::info!("[MockNet] Registering node {node_id} at prefix: {prefix}");

        let mut inner = self.inner.lock().unwrap();
        if inner.nodes.insert(*node_id, prefix.to_string()).is_some() {
            panic!("[MockNet] Node [{}] already existed.", node_id);
        }
    }

    fn unregister_node(&self, node_id: &NodeId) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .nodes
            .remove(node_id)
            .map(|_| ())
            .ok_or_else(|| anyhow::anyhow!("node not registered: {}", node_id))
    }
}

// TODO: all tests using this mock net implementation should be run sequentially
// because GSB router is a static singleton (shared state) and consecutive bindings
// for same addr (ie. local_net::BUS_ID) are being overwritten and only last is effective
// which means there might be interlace in BCastService instances being used
// `bcast::singleton` is a try to handle it, but unsuccessful yet
impl MockNet {
    pub fn new() -> Self {
        MockNet {
            inner: Arc::new(Mutex::new(MockNetInner::default())),
            broadcast: Default::default(),
        }
    }
    pub fn bind(self) -> Self {
        self.bind_gsb();
        self
    }

    fn translate_to(&self, id: NodeId, addr: &str) -> Result<String> {
        let prefix = self.node_prefix(id)?;
        let net_prefix = format!("/net/{}", id);

        log::debug!("Replacing {net_prefix} with {prefix} in {addr}");
        Ok(addr.replacen(&net_prefix, &prefix, 1))
    }

    fn node_prefix(&self, id: NodeId) -> Result<String> {
        let inner = self.inner.lock().unwrap();
        inner
            .nodes
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Node not registered: {id}"))
    }

    pub fn node_by_prefix(&self, address: &str) -> Option<NodeId> {
        let inner = self.inner.lock().unwrap();
        for (id, prefix) in inner.nodes.iter() {
            if address.contains(prefix) {
                return Some(*id);
            }
        }
        None
    }

    fn bind_gsb_inner(&self) {
        log::info!("initializing BCast on mock net");

        let bcast_service_id = <SendBroadcastMessage<()> as RpcMessage>::ID;

        let bcast = self.broadcast.clone();
        let bcast1 = self.broadcast.clone();
        let _ = bus::bind(local_net::BUS_ID, move |subscribe: local_net::Subscribe| {
            let bcast = bcast1.clone();
            async move {
                log::debug!("subscribing BCast: {:?}", subscribe);
                bcast.subscribe_topic(subscribe);
                Ok(0) // ignored id
            }
        });

        let mock_net = self.clone();

        let addr = format!("{}/{}", local_net::BUS_ID, bcast_service_id);
        let resp: Rc<[u8]> = serialization::to_vec(&Ok::<(), ()>(())).unwrap().into();
        let _ = local_bus::subscribe(
            &addr,
            move |caller: &str, _addr: &str, msg: &[u8]| {
                let resp = resp.clone();
                let bcast = bcast.clone();

                let stub: SendBroadcastStub = serialization::from_slice(msg).unwrap();
                let caller = caller.to_string();

                let msg = msg.to_vec();

                let topic = stub.topic;
                let endpoints = bcast.resolve(&caller, &topic);

                log::debug!("BCasting on {topic} to {endpoints:?} from {caller}");
                for endpoint in endpoints {
                    let addr = format!("{endpoint}/{bcast_service_id}");

                    // Normal net would have additional step: Broadcast message would be sent to other node first on /net/{node_id}.
                    // Net would receive message, check topic and translate it to local addresses interested in this topic.
                    // Here for simplicity we are skipping those additional steps and directly sending to all endpoints waiting for broadcast.
                    //
                    // But since all broadcast handlers are bound on `/local` and all addresses registered in net are on `/public`,
                    // we must replace `local` -> `public` to find NodeId of receiver.
                    let addr_local = addr.replacen("local", "public", 1);

                    let node_id = match mock_net.node_by_prefix(&addr_local) {
                        Some(node_id) => node_id,
                        None => {
                            log::debug!(
                                "Not broadcasting on topic {topic} to {addr}. Node not found on list. \
                                Probably networking was disabled for this Node."
                            );
                            continue;
                        }
                    };

                    log::debug!("BCasting on {topic} to address: {addr}, node: [{node_id}]");

                    let caller = caller.clone();
                    let msg = msg.clone();
                    tokio::task::spawn_local(async move {
                        let _ = local_bus::send(addr.as_ref(), &caller, msg.as_ref()).await;
                    });
                }
                async move { Ok(Vec::from(resp.as_ref())) }
            },
            (),
        );

        Self::bind_local_bus(self.clone(), FROM_BUS_ID, from_address_resolver);
        Self::bind_local_bus(self.clone(), FROM_UDP_BUS_ID, from_address_resolver);
        Self::bind_local_bus(self.clone(), FROM_TRANSFER_BUS_ID, from_address_resolver);

        Self::bind_local_bus(self.clone(), net::BUS_ID, net_address_resolver);
        Self::bind_local_bus(self.clone(), net::BUS_ID_UDP, net_address_resolver);
        Self::bind_local_bus(self.clone(), net::BUS_ID_TRANSFER, net_address_resolver);
    }

    fn bind_local_bus<F>(net: MockNet, address: &'static str, resolver: F)
    where
        F: Fn(&str, &str) -> anyhow::Result<(String, NodeId, String)> + 'static,
    {
        let resolver = Arc::new(resolver);

        local_bus::subscribe(
            address,
            move |caller: &str, addr: &str, msg: &[u8]| {
                let mock_net = net.clone();
                let data = Vec::from(msg);
                let caller = caller.to_string();
                let addr = addr.to_string();
                let resolver_ = resolver.clone();

                async move {
                    log::info!("[MockNet] Received message from [{caller}], on address [{addr}].");

                    let (from, to, address) = resolver_(&caller, &addr)
                        .map_err(|e| Error::GsbBadRequest(e.to_string()))?;
                    let translated = mock_net
                        .translate_to(to, &address)
                        .map_err(|e| Error::GsbBadRequest(e.to_string()))?;

                    log::info!(
                        "[MockNet] Sending message from [{from}], to: [{to}], address [{translated}]."
                    );
                    local_bus::send(&translated, &from.to_string(), &data).await
                }
            },
            // TODO: Implement stream handler
            (),
        );
    }
}

fn from_address_resolver(_caller: &str, addr: &str) -> anyhow::Result<(String, NodeId, String)> {
    let (from, to, addr) =
        parse_from_to_addr(addr).map_err(|e| anyhow::anyhow!("invalid address: {}", e))?;
    Ok((from.to_string(), to, addr))
}

fn net_address_resolver(caller: &str, addr: &str) -> anyhow::Result<(String, NodeId, String)> {
    let (to, addr) =
        parse_net_to_addr(addr).map_err(|e| anyhow::anyhow!("invalid address: {}", e))?;
    Ok((caller.to_string(), to, addr))
}

pub(crate) const FROM_BUS_ID: &str = "/from";
pub(crate) const FROM_UDP_BUS_ID: &str = "/udp/from";
pub(crate) const FROM_TRANSFER_BUS_ID: &str = "/transfer/from";
