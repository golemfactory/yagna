use actix_rt::Arbiter;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use ya_core_model::net::{local as local_net, local::SendBroadcastMessage};
use ya_service_bus::{typed as bus, untyped as local_bus, RpcMessage};

#[cfg(feature = "bcast-singleton")]
use super::bcast::singleton::BCastService;
use super::bcast::BCast;
#[cfg(not(feature = "bcast-singleton"))]
use super::bcast::BCastService;

#[derive(Clone)]
pub struct MockNet {
    inner: Arc<Mutex<MockNetInner>>,
}

struct MockNetInner;

lazy_static::lazy_static! {
    static ref NET : MockNet = MockNet {
        inner: Arc::new(Mutex::new(MockNetInner))
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
        let me = self.inner.lock().unwrap();
        me.bind_gsb()
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

        let bcast_service_id = <SendBroadcastMessage<serde_json::Value> as RpcMessage>::ID;

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
        let resp: Rc<[u8]> = serde_json::to_vec(&Ok::<(), ()>(())).unwrap().into();
        let _ = local_bus::subscribe(&addr, move |caller: &str, _addr: &str, msg: &[u8]| {
            let resp = resp.clone();
            let bcast = bcast.clone();

            let msg_json: SendBroadcastMessage<serde_json::Value> =
                serde_json::from_slice(msg).unwrap();
            let caller = caller.to_string();

            let msg = serde_json::to_vec(&msg_json).unwrap();
            let topic = msg_json.topic().to_owned();
            let endpoints = bcast.resolve(&caller, &topic);

            log::debug!("BCasting on {} to {:?} from {}", topic, endpoints, caller);
            for endpoint in endpoints {
                let addr = format!("{}/{}", endpoint, bcast_service_id);
                log::debug!("BCasting on {} to {}", topic, addr);
                let caller = caller.clone();
                let msg = msg.clone();
                Arbiter::spawn(async move {
                    let _ = local_bus::send(addr.as_ref(), &caller, msg.as_ref()).await;
                });
            }
            async move { Ok(Vec::from(resp.as_ref())) }
        });
    }
}
