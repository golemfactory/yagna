use anyhow::Result;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use ya_client::model::NodeId;
use ya_core_model::net;
use ya_core_model::net::{local as local_net, local::SendBroadcastMessage};
use ya_net::hybrid::testing::{parse_from_to_addr, parse_net_to_addr};
use ya_service_bus::{serialization, typed as bus, untyped as local_bus, Error, RpcMessage};

#[cfg(feature = "bcast-singleton")]
use super::bcast::singleton::BCastService;
use super::bcast::BCast;
#[cfg(not(feature = "bcast-singleton"))]
use super::bcast::BCastService;
use ya_core_model::net::local::SendBroadcastStub;

#[derive(Clone)]
pub struct MockNet {
    inner: Arc<Mutex<MockNetInner>>,
}

#[derive(Default)]
struct MockNetInner {
    /// Maps NodeIds to gsb prefixes of market nodes.
    pub nodes: HashMap<NodeId, String>,
}

lazy_static::lazy_static! {
    static ref NET : MockNet = MockNet {
        inner: Arc::new(Mutex::new(MockNetInner::default()))
    };
}

impl Default for MockNet {
    fn default() -> Self {
        log::debug!("getting singleton MockNet");
        (*NET).clone()
    }
}

impl MockNet {
    pub fn bind_gsb(&self) {
        let inner = self.inner.lock().unwrap();
        inner.bind_gsb()
    }

    pub fn register_node(&self, node_id: &NodeId, prefix: &str) {
        log::info!("[MockNet] Registering node {node_id} at prefix: {prefix}");

        let mut inner = self.inner.lock().unwrap();
        if inner.nodes.insert(*node_id, prefix.to_string()).is_some() {
            panic!("[MockNet] Node [{}] already existed.", node_id);
        }
    }

    pub fn unregister_node(&self, node_id: &NodeId) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .nodes
            .remove(node_id)
            .map(|_| ())
            .ok_or_else(|| anyhow::anyhow!("node not registered: {}", node_id))
    }

    fn translate_to(&self, id: NodeId, addr: &str) -> Result<String> {
        let prefix = self.node_prefix(id)?;
        let net_prefix = format!("/net/{}", id);
        Ok(addr.replacen(&net_prefix, &prefix, 1))
    }

    fn node_prefix(&self, id: NodeId) -> Result<String> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .nodes
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("node not registered: {}", id))?)
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
}

// TODO: all tests using this mock net implementation should be run sequentially
// because GSB router is a static singleton (shared state) and consecutive bindings
// for same addr (ie. local_net::BUS_ID) are being overwritten and only last is effective
// which means there might be interlace in BCastService instances being used
// `bcast::singleton` is a try to handle it, but unsuccessful yet
impl MockNetInner {
    pub fn bind_gsb(&self) {
        let bcast = BCastService::default();
        log::info!("initializing BCast on mock net");

        let bcast_service_id = <SendBroadcastMessage<()> as RpcMessage>::ID;

        let bcast1 = bcast.clone();
        let _ = bus::bind(local_net::BUS_ID, move |subscribe: local_net::Subscribe| {
            let bcast = bcast1.clone();
            async move {
                log::debug!("subscribing BCast: {:?}", subscribe);
                bcast.add(subscribe);
                Ok(0) // ignored id
            }
        });

        let addr = format!("{}/{}", local_net::BUS_ID, bcast_service_id);
        let resp: Rc<[u8]> = serialization::to_vec(&Ok::<(), ()>(())).unwrap().into();
        let _ = local_bus::subscribe(
            &addr,
            move |caller: &str, _addr: &str, msg: &[u8]| {
                let mock_net = MockNet::default();
                let resp = resp.clone();
                let bcast = bcast.clone();

                let stub: SendBroadcastStub = serialization::from_slice(msg).unwrap();
                let caller = caller.to_string();

                let msg = msg.to_vec();

                let topic = stub.topic;
                let endpoints = bcast.resolve(&caller, &topic);

                log::debug!("BCasting on {} to {:?} from {}", topic, endpoints, caller);
                for endpoint in endpoints {
                    let addr = format!("{}/{}", endpoint, bcast_service_id);

                    let node_id = match mock_net.node_by_prefix(&addr) {
                        Some(node_id) => node_id,
                        None => {
                            log::debug!(
                                "Not broadcasting on topic {} to {}. Node not found on list. \
                                Probably networking was disabled for this Node.",
                                topic,
                                addr
                            );
                            continue;
                        }
                    };

                    log::debug!(
                        "BCasting on {} to address: {}, node: [{}]",
                        topic,
                        addr,
                        node_id
                    );
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

        Self::bind_local_bus(MockNet::default(), FROM_BUS_ID, from_address_resolver);
        Self::bind_local_bus(MockNet::default(), FROM_UDP_BUS_ID, from_address_resolver);
        Self::bind_local_bus(
            MockNet::default(),
            FROM_TRANSFER_BUS_ID,
            from_address_resolver,
        );

        Self::bind_local_bus(MockNet::default(), net::BUS_ID, net_address_resolver);
        Self::bind_local_bus(MockNet::default(), net::BUS_ID_UDP, net_address_resolver);
        Self::bind_local_bus(
            MockNet::default(),
            net::BUS_ID_TRANSFER,
            net_address_resolver,
        );
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

pub fn gsb_prefixes(test_name: &str, name: &str) -> (String, String) {
    let public_gsb_prefix = format!("/{}/{}/market", test_name, name);
    let local_gsb_prefix = format!("/{}/{}/market", test_name, name);
    (public_gsb_prefix, local_gsb_prefix)
}
