use crate::SUBSCRIPTIONS;
use actix_rt::Arbiter;
use futures::channel::oneshot;
use futures::{future, Future, FutureExt, StreamExt, TryFutureExt};
use metrics::{counter, timing};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use ya_core_model::net;
use ya_core_model::net::local::BindBroadcastError;
use ya_service_bus::connection::CallRequestHandler;
use ya_service_bus::{typed as bus, Error, ResponseChunk, RpcEndpoint};

pub struct CentralBusHandler<C, E> {
    call_handler: C,
    event_handler: E,
    tx: Option<oneshot::Sender<()>>,
}

impl<C, E> CentralBusHandler<C, E> {
    pub fn new(call_handler: C, event_handler: E) -> (Self, oneshot::Receiver<()>) {
        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        counter!("net.connect", 0);
        counter!("net.disconnect", 0);

        let (tx, rx) = oneshot::channel();
        (
            Self {
                call_handler,
                event_handler,
                tx: Some(tx),
            },
            rx,
        )
    }
}

impl<C, E, S> CallRequestHandler for CentralBusHandler<C, E>
where
    C: FnMut(String, String, String, Vec<u8>) -> S,
    E: FnMut(String, String, Vec<u8>),
    S: futures::Stream<Item = Result<ResponseChunk, Error>> + Unpin,
{
    type Reply = S;

    fn do_call(
        &mut self,
        request_id: String,
        caller: String,
        address: String,
        data: Vec<u8>,
    ) -> Self::Reply {
        (self.call_handler)(request_id, caller, address, data)
    }

    fn handle_event(&mut self, caller: String, topic: String, data: Vec<u8>) {
        (self.event_handler)(caller, topic, data)
    }

    fn on_disconnect(&mut self) {
        self.tx.take().map(|tx| tx.send(()));
    }
}

#[inline]
pub(crate) async fn auto_rebind<B, U, Fb, Fu, Fr, E>(bind: B, unbind: U) -> anyhow::Result<()>
where
    B: FnMut() -> Fb + 'static,
    U: FnMut() -> Fu + 'static,
    Fb: Future<Output = std::io::Result<Fr>> + 'static,
    Fu: Future<Output = ()> + 'static,
    Fr: Future<Output = Result<(), E>> + 'static,
    E: 'static,
{
    Ok(rebind(Default::default(), bind, Rc::new(RefCell::new(unbind))).await?)
}

async fn rebind<B, U, Fb, Fu, Fr, E>(
    reconnect: Rc<RefCell<ReconnectContext>>,
    mut bind: B,
    unbind: Rc<RefCell<U>>,
) -> anyhow::Result<()>
where
    B: FnMut() -> Fb + 'static,
    U: FnMut() -> Fu + 'static,
    Fb: Future<Output = std::io::Result<Fr>> + 'static,
    Fu: Future<Output = ()> + 'static,
    Fr: Future<Output = Result<(), E>> + 'static,
    E: 'static,
{
    let (tx, rx) = oneshot::channel();
    let unbind_clone = unbind.clone();

    loop {
        match bind().await {
            Ok(dc_rx) => {
                if let Some(start) = reconnect.borrow_mut().last_disconnect {
                    let end = Instant::now();
                    timing!("net.reconnect.time", start, end);
                }
                reconnect.replace(Default::default());
                resubscribe().await;
                counter!("net.connect", 1);

                let reconnect_clone = reconnect.clone();
                Arbiter::spawn(async move {
                    if let Ok(_) = dc_rx.await {
                        counter!("net.disconnect", 1);
                        reconnect_clone.borrow_mut().last_disconnect = Some(Instant::now());
                        log::warn!("Handlers disconnected");
                        (*unbind_clone.borrow_mut())().await;
                        let _ = tx.send(());
                    }
                });
                break;
            }
            Err(error) => {
                let delay = { reconnect.borrow_mut().next().unwrap() };
                log::warn!(
                    "Failed to bind handlers: {}; retrying in {} s",
                    error,
                    delay.as_secs_f32()
                );
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        return Err(anyhow::anyhow!("Net initialization interrupted"));
                    },
                    _ = tokio::time::delay_for(delay) => {},
                }
            }
        }
    }

    Arbiter::spawn(rx.then(move |_| rebind(reconnect, bind, unbind).then(|_| future::ready(()))));
    Ok(())
}

async fn resubscribe() {
    futures::stream::iter({ SUBSCRIPTIONS.lock().unwrap().clone() }.into_iter())
        .for_each(|msg| {
            let topic = msg.topic().to_owned();
            async move {
                Ok::<_, BindBroadcastError>(bus::service(net::local::BUS_ID).send(msg).await??)
            }
            .map_err(move |e| log::error!("Failed to subscribe {}: {}", topic, e))
            .then(|_| futures::future::ready(()))
        })
        .await;
}

pub struct ReconnectContext {
    pub current: f32, // s
    pub max: f32,     // s
    pub factor: f32,
    pub last_disconnect: Option<Instant>,
}

impl Iterator for ReconnectContext {
    type Item = Duration;

    fn next(&mut self) -> Option<Self::Item> {
        self.current = self.max.min(self.current * self.factor);
        Some(Duration::from_secs_f32(self.current))
    }
}

impl Default for ReconnectContext {
    fn default() -> Self {
        ReconnectContext {
            current: 1.,
            max: 1800.,
            factor: 2.,
            last_disconnect: None,
        }
    }
}
