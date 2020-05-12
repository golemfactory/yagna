use futures::prelude::*;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

use ya_service_bus::RpcMessage;

/// Object for storing callback functions.
pub struct HandlerSlot<MsgType: RpcMessage> {
    slot: Arc<Mutex<Box<dyn CallbackHandler<MsgType>>>>,
}

impl<MsgType: RpcMessage> HandlerSlot<MsgType> {
    pub fn new(callback: impl CallbackHandler<MsgType>) -> HandlerSlot<MsgType> {
        HandlerSlot {
            slot: Arc::new(Mutex::new(Box::new(callback))),
        }
    }

    pub async fn call(&self, caller: String, msg: MsgType) -> CallbackResult<MsgType> {
        // Handle will return future under lock. It shouldn't take to much time.
        let future = { self.slot.lock().await.handle(caller, msg) };
        // The biggest work is done here in this await. But we already
        // freed lock.
        future.await
    }
}

/// Trait to use in functions for binding callbacks.
/// Example:
/// ```rust
/// use ya_service_bus::RpcMessage;
/// use ya_market_decentralized::protocol::{CallbackHandler, HandlerSlot};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Serialize, Deserialize)]
/// struct GenericMessage;
///
/// impl RpcMessage for GenericMessage {
///     const ID :&'static str = "GenericMessage";
///     type Item = String;
///     type Error=();
/// }
///
/// fn bind(callback: impl CallbackHandler<GenericMessage>) {
///     let slot = HandlerSlot::<GenericMessage>::new(callback);
///     // Slot can be called later like this:
///     //slot.call(format!("caller-id"), GenericMessage{}).await
/// }
/// ```
pub trait CallbackHandler<MsgType: RpcMessage>: Send + Sync + 'static {
    fn handle(&mut self, caller: String, msg: MsgType) -> CallbackFuture<MsgType>;
}

pub type CallbackFuture<MsgType> = Pin<Box<dyn OutputFuture<MsgType>>>;
pub type CallbackResult<MsgType> =
    Result<<MsgType as RpcMessage>::Item, <MsgType as RpcMessage>::Error>;

/// Implements callback handler for FnMut to enable passing
/// lambdas and other functions to handlers.
impl<
        MsgType: RpcMessage,
        Output: OutputFuture<MsgType>,
        F: FnMut(MsgType) -> Output + Send + Sync + 'static,
    > CallbackHandler<MsgType> for F
{
    fn handle(&mut self, _caller: String, msg: MsgType) -> CallbackFuture<MsgType> {
        Box::pin(self(msg))
    }
}

/// Shortcut for writing complicated Future signature.
pub trait OutputFuture<MsgType: RpcMessage>: Send + Sync + 'static
where
    Self: Future<Output = CallbackResult<MsgType>> + Send + Sync + 'static,
{
}

/// All futures will implement OutputFuture.
impl<T, MsgType> OutputFuture<MsgType> for T
where
    MsgType: RpcMessage,
    T: Future<Output = CallbackResult<MsgType>> + Send + Sync + 'static,
{
}
