use actix_rt::Arbiter;
use std::rc::Rc;

use ya_core_model::net::{local as local_net, local::SendBroadcastMessage};
use ya_service_bus::{typed as bus, untyped as local_bus, RpcMessage};

use super::bcast;

pub struct MockNet;

// TODO: all tests using this mock net implementation should be run sequentially
// because GSB router is a static singleton (shared state) and consecutive bindings
// for same addr (ie. local_net::BUS_ID) are being overwritten and only last is effective
// which means there might be interlace in BCastService instances being used
// `bcast_singleton.rs` is a try to handle it, but unsuccessful yet
impl MockNet {
    pub fn gsb(bcast: bcast::BCastService) -> anyhow::Result<()> {
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

        Ok(())
    }
}
