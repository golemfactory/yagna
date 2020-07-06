use actix_rt::Arbiter;
use anyhow::bail;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use ya_client::model::NodeId;
use ya_core_model::net;
use ya_core_model::net::{local as local_net, local::SendBroadcastMessage};
use ya_service_bus::{typed as bus, untyped as local_bus, Error, RpcMessage};

use super::bcast;

#[derive(Clone)]
pub struct MockNet {
    inner: Arc<Mutex<MockNetImpl>>,
}

struct MockNetImpl {
    /// Maps NodeIds to gsb prefixes of market nodes.
    pub nodes: HashMap<NodeId, String>,
}

// TODO: all tests using this mock net implementation should be run sequentially
// because GSB router is a static singleton (shared state) and consecutive bindings
// for same addr (ie. local_net::BUS_ID) are being overwritten and only last is effective
// which means there might be interlace in BCastService instances being used
// `bcast_singleton.rs` is a try to handle it, but unsuccessful yet
impl MockNet {
    pub fn new() -> Result<MockNet, anyhow::Error> {
        let inner = MockNetImpl {
            nodes: HashMap::new(),
        };
        let net = MockNet {
            inner: Arc::new(Mutex::new(inner)),
        };

        net.gsb()?;
        Ok(net)
    }

    pub fn gsb_prefixes(&self, test_name: &str, name: &str) -> (String, String) {
        let public_gsb_prefix = format!("/{}/{}/market", test_name, name);
        let local_gsb_prefix = format!("/{}/{}/market", test_name, name);
        (public_gsb_prefix, local_gsb_prefix)
    }

    pub async fn register_node(&self, node_id: &NodeId, prefix: &str) -> anyhow::Result<()> {
        // Only two first components
        let mut iter = prefix.split("/").fuse();
        let prefix = match (iter.next(), iter.next(), iter.next()) {
            (Some(""), Some(test_name), Some(name)) => format!("/{}/{}", test_name, name),
            _ => bail!("[MockNet] Can't register prefix {}", prefix),
        };

        self.inner
            .lock()
            .await
            .nodes
            .insert(node_id.clone(), prefix);
        Ok(())
    }

    pub fn gsb(&self) -> anyhow::Result<()> {
        let bcast = bcast::BCastService::default();
        log::info!("initializing BCast on mock net");

        let bcast_service_id = <SendBroadcastMessage<serde_json::Value> as RpcMessage>::ID;

        {
            let bcast = bcast.clone();
            let _ = bus::bind(local_net::BUS_ID, move |subscribe: local_net::Subscribe| {
                let bcast = bcast.clone();
                async move {
                    log::debug!("subscribing BCast: {:?}", subscribe);
                    bcast.add(subscribe);
                    Ok(0) // ignored id
                }
            });
        }

        {
            let bcast = bcast.clone();
            let addr = format!("{}/{}", local_net::BUS_ID, bcast_service_id);
            let resp: Rc<[u8]> = serde_json::to_vec(&Ok::<(), ()>(())).unwrap().into();
            let _ = local_bus::subscribe(&addr, move |caller: &str, _addr: &str, msg: &[u8]| {
                let resp = resp.clone();
                let bcast = bcast.clone();

                let msg_json: SendBroadcastMessage<serde_json::Value> =
                    serde_json::from_slice(msg).unwrap();
                let caller = caller.to_string();

                Arbiter::spawn(async move {
                    let msg = serde_json::to_vec(&msg_json).unwrap();
                    let topic = msg_json.topic().to_owned();
                    let endpoints = bcast.resolve(&topic);

                    log::debug!("BCasting on {} to {:?} from {}", topic, endpoints, caller);
                    for endpoint in endpoints {
                        let addr = format!("{}/{}", endpoint, bcast_service_id);
                        let _ = local_bus::send(addr.as_ref(), &caller, msg.as_ref()).await;
                    }
                });
                async move { Ok(Vec::from(resp.as_ref())) }
            });
        }

        {
            let mock_net = self.clone();
            local_bus::subscribe(FROM_BUS_ID, move |caller: &str, addr: &str, msg: &[u8]| {
                let mock_net = mock_net.clone();
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
            });
        }

        Ok(())
    }

    async fn translate_address(&self, address: String) -> Result<(NodeId, String), anyhow::Error> {
        let (from_node, to_addr) = match parse_from_addr(&address) {
            Ok(v) => v,
            Err(e) => Err(Error::GsbBadRequest(e.to_string()))?,
        };

        let mut iter = to_addr.split("/").fuse();
        let dst_id = match (iter.next(), iter.next(), iter.next()) {
            (Some(""), Some("net"), Some(dst_id)) => dst_id,
            _ => bail!("[MockNet] Invalid destination address {}", to_addr),
        };

        let dest_node_id = NodeId::from_str(&dst_id)?;
        let inner = self.inner.lock().await;
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
}

// Copied from core/net/api.rs
pub(crate) fn parse_from_addr(from_addr: &str) -> anyhow::Result<(NodeId, String)> {
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
    bail!("invalid net-from destination: {}", from_addr)
}

// Copied from core/net/api.rs
#[inline]
pub(crate) fn net_service(service: impl ToString) -> String {
    format!("{}/{}", net::BUS_ID, service.to_string())
}

pub(crate) const FROM_BUS_ID: &str = "/from";
