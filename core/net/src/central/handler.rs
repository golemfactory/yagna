use futures::channel::oneshot;
use ya_service_bus::connection::CallRequestHandler;
use ya_service_bus::{Error, ResponseChunk};

pub struct CentralBusHandler<C, E> {
    call_handler: C,
    event_handler: E,
    tx: Option<oneshot::Sender<()>>,
}

impl<C, E> CentralBusHandler<C, E> {
    pub fn new(call_handler: C, event_handler: E) -> (Self, oneshot::Receiver<()>) {
        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        metrics::counter!("net.connect", 0);
        metrics::counter!("net.disconnect", 0);

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
        _no_reply: bool,
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
