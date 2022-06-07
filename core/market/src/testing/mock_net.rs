use anyhow::Result;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use ya_client::model::NodeId;
use ya_core_model::net;
use ya_core_model::net::{local as local_net, local::SendBroadcastMessage};
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
        // Only two first components
        let mut iter = prefix.split("/").fuse();
        let prefix = match (iter.next(), iter.next(), iter.next()) {
            (Some(""), Some(test_name), Some(name)) => format!("/{}/{}", test_name, name),
            _ => panic!("[MockNet] Can't register prefix {}", prefix),
        };

        let mut inner = self.inner.lock().unwrap();
        if let Some(_) = inner.nodes.insert(node_id.clone(), prefix) {
            panic!("[MockNet] Node [{}] already existed.", &node_id);
        }
    }

    pub fn unregister_node(&self, node_id: &NodeId) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .nodes
            .remove(node_id)
            .map(|_| ())
            .ok_or(anyhow::anyhow!("node not registered: {}", node_id))
    }

    async fn translate_address(&self, address: String) -> Result<(NodeId, String)> {
        let (from_node, to_addr) = match parse_from_addr(&address) {
            Ok(v) => v,
            Err(e) => Err(Error::GsbBadRequest(e.to_string()))?,
        };

        let mut iter = to_addr.split("/").fuse();
        let dst_id = match (iter.next(), iter.next(), iter.next()) {
            (Some(""), Some("net"), Some(dst_id)) => dst_id,
            _ => panic!("[MockNet] Invalid destination address {}", to_addr),
        };

        let dest_node_id = NodeId::from_str(&dst_id)?;
        let inner = self.inner.lock().unwrap();
        let local_prefix = inner.nodes.get(&dest_node_id);

        if let Some(local_prefix) = local_prefix {
            let net_prefix = format!("/net/{}", dst_id);
            Ok((from_node, to_addr.replacen(&net_prefix, &local_prefix, 1)))
        } else {
            Err(Error::GsbFailure(format!(
                "[MockNet] Can't find destination address for endpoint [{}].",
                &address
            )))?
        }
    }

    pub fn node_by_prefix(&self, address: &str) -> Option<NodeId> {
        let inner = self.inner.lock().unwrap();
        for (id, prefix) in inner.nodes.iter() {
            if address.contains(prefix) {
                return Some(id.clone());
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

                let msg = msg.iter().copied().collect::<Vec<_>>();

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

        local_bus::subscribe(
            FROM_BUS_ID,
            move |caller: &str, addr: &str, msg: &[u8]| {
                let mock_net = MockNet::default();
                let data = Vec::from(msg);
                let caller = caller.to_string();
                let addr = addr.to_string();

                async move {
                    let (from, local_addr) = mock_net
                        .translate_address(addr)
                        .await
                        .map_err(|e| Error::GsbBadRequest(e.to_string()))?;

                    log::debug!(
                        "[MockNet] Sending message from [{}], to address [{}].",
                        &caller,
                        &local_addr
                    );
                    Ok(local_bus::send(&local_addr, &from.to_string(), &data).await?)
                }
            },
            (),
        );
    }
}

// Copied from core/net/api.rs
pub(crate) fn parse_from_addr(from_addr: &str) -> Result<(NodeId, String)> {
    let mut it = from_addr.split("/").fuse();
    if let (Some(""), Some("from"), Some(from_node_id), Some("to"), Some(to_node_id)) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    {
        to_node_id.parse::<NodeId>()?;
        let prefix = 10 + from_node_id.len();
        let service_id = &from_addr[prefix..];
        if let Some(_) = it.next() {
            return Ok((from_node_id.parse()?, net_service(service_id)));
        }
    }
    anyhow::bail!("invalid net-from destination: {}", from_addr)
}

// Copied from core/net/api.rs
#[inline]
pub(crate) fn net_service(service: impl ToString) -> String {
    format!("{}/{}", net::BUS_ID, service.to_string())
}

pub(crate) const FROM_BUS_ID: &str = "/from";

pub fn gsb_prefixes(test_name: &str, name: &str) -> (String, String) {
    let public_gsb_prefix = format!("/{}/{}/market", test_name, name);
    let local_gsb_prefix = format!("/{}/{}/market", test_name, name);
    (public_gsb_prefix, local_gsb_prefix)
}
