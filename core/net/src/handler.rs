use actix_rt::Arbiter;
use futures::channel::oneshot;
use futures::{future, Future, FutureExt};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use ya_service_bus::connection::CallRequestHandler;
use ya_service_bus::{Error, ResponseChunk};

pub struct CentralBusHandler<F1, F2> {
    call_handler: F1,
    event_handler: F2,
    tx: Option<oneshot::Sender<()>>,
}

impl<F1, F2> CentralBusHandler<F1, F2> {
    pub fn new(call_handler: F1, event_handler: F2) -> (Self, oneshot::Receiver<()>) {
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

impl<R, F1, F2> CallRequestHandler for CentralBusHandler<F1, F2>
where
    R: futures::Stream<Item = Result<ResponseChunk, Error>> + Unpin,
    F1: FnMut(String, String, String, Vec<u8>) -> R,
    F2: FnMut(String, String, Vec<u8>),
{
    type Reply = R;

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

pub(crate) async fn auto_rebind<F, Fut>(f: F)
where
    F: FnMut() -> Fut + 'static,
    Fut: Future<Output = std::io::Result<oneshot::Receiver<()>>> + 'static,
{
    rebind(Default::default(), f).await
}

async fn rebind<F, Fut>(reconnect: Rc<RefCell<ReconnectContext>>, mut f: F)
where
    F: FnMut() -> Fut + 'static,
    Fut: Future<Output = std::io::Result<oneshot::Receiver<()>>> + 'static,
{
    let (tx, rx) = oneshot::channel();

    loop {
        match f().await {
            Ok(dc_rx) => {
                Arbiter::spawn(async move {
                    if let Ok(_) = dc_rx.await {
                        log::warn!("Handlers disconnected");
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
                tokio::time::delay_for(delay).await;
            }
        }
    }
    Arbiter::spawn(rx.then(move |_| rebind(reconnect, f).then(|_| future::ready(()))));
}

pub struct ReconnectContext {
    pub current: f32, // s
    pub max: f32,     // s
    pub factor: f32,
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
        }
    }
}
