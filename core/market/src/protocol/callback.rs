use futures::prelude::*;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Implement for callback message parameter.
pub trait CallbackMessage: Serialize + DeserializeOwned + 'static + Sync + Send {
    type Ok: Serialize + DeserializeOwned + 'static + Sync + Send;
    type Error: Serialize + DeserializeOwned + 'static + Sync + Send + Debug;
}

/// Object for storing callback functions.
#[derive(Clone)]
pub struct HandlerSlot<MsgType: CallbackMessage> {
    slot: Arc<Mutex<Box<dyn CallbackHandler<MsgType>>>>,
}

impl<MsgType: CallbackMessage> HandlerSlot<MsgType> {
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
/// use ya_market::testing::callback::{CallbackHandler, HandlerSlot, CallbackMessage};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Serialize, Deserialize)]
/// struct GenericMessage;
///
/// impl CallbackMessage for GenericMessage {
///     type Ok = String;
///     type Error=();
/// }
///
/// fn bind(callback: impl CallbackHandler<GenericMessage>) {
///     let slot = HandlerSlot::<GenericMessage>::new(callback);
///     // Slot can be called later like this:
///     //slot.call(format!("caller-id"), GenericMessage{}).await
/// }
/// ```
pub trait CallbackHandler<MsgType: CallbackMessage>: Send + Sync + 'static {
    fn handle(&mut self, caller: String, msg: MsgType) -> CallbackFuture<MsgType>;
}

pub type CallbackFuture<MsgType> = Pin<Box<dyn OutputFuture<MsgType>>>;
pub type CallbackResult<MsgType> =
    Result<<MsgType as CallbackMessage>::Ok, <MsgType as CallbackMessage>::Error>;

/// Implements callback handler for FnMut to enable passing
/// lambdas and other functions to handlers.
impl<
        MsgType: CallbackMessage,
        Output: OutputFuture<MsgType>,
        F: FnMut(String, MsgType) -> Output + Send + Sync + 'static,
    > CallbackHandler<MsgType> for F
{
    fn handle(&mut self, caller: String, msg: MsgType) -> CallbackFuture<MsgType> {
        Box::pin(self(caller, msg))
    }
}

/// Shortcut for writing complicated Future signature.
pub trait OutputFuture<MsgType: CallbackMessage>: Send + Sync + 'static
where
    Self: Future<Output = CallbackResult<MsgType>> + Send + Sync + 'static,
{
}

/// All futures will implement OutputFuture.
impl<T, MsgType> OutputFuture<MsgType> for T
where
    MsgType: CallbackMessage,
    T: Future<Output = CallbackResult<MsgType>> + Send + Sync + 'static,
{
}
