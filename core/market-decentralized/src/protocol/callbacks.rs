use futures::prelude::*;
use std::pin::Pin;

use ya_service_bus::RpcMessage;


/// Object for storing callback functions.
pub struct HandlerSlot<MsgType: RpcMessage>
{
    slot: Box<dyn CallbackHandler<MsgType>>,
}

impl<MsgType: RpcMessage> HandlerSlot<MsgType>
{
    pub fn new(callback: impl CallbackHandler<MsgType>) -> HandlerSlot<MsgType> {
        HandlerSlot{ slot: Box::new(callback) }
    }
}

/// Trait to use in functions for binding callbacks.
/// Example:
/// ```rust
/// use ya_service_bus::RpcMessage;
///
/// struct GenericMessage;
/// impl RpcMessage for GenericMessage {
///     const ID :&'static str = "GenericMessage";
///     type Item = String;
///     type Error=();
/// }
///
/// fn bind(callback: impl CallbackHandler<GenericMessage>) {
///     let slot = HandlerSlot::<GenericMessage>::new(callback);
/// }
/// ```
pub trait CallbackHandler<MsgType: RpcMessage>: 'static {
    fn handle(&mut self, caller: String, msg: MsgType) -> CallbackResult<MsgType>;
}

pub type CallbackResult<MsgType> = Pin<Box<dyn OutputFuture<MsgType>>>;

/// Implements callback handler for FnMut to enable passing
/// lambdas and other functions to handlers.
impl<
    MsgType: RpcMessage,
    Output: Future<Output = Result<MsgType::Item, MsgType::Error>> + 'static,
    F: FnMut(MsgType) -> Output + 'static,
> CallbackHandler<MsgType> for F
{
    fn handle(&mut self, _caller: String, msg: MsgType) -> CallbackResult<MsgType> {
        Box::pin(self(msg))
    }
}

/// Shortcut for writing complicated Future signature.
pub trait OutputFuture<MsgType: RpcMessage>
    where
        Self: Future<Output = Result<<MsgType as RpcMessage>::Item, <MsgType as RpcMessage>::Error>> + 'static
{}

/// All futures will implement OutputFuture.
impl<T, MsgType> OutputFuture<MsgType> for T
    where
        MsgType: RpcMessage,
        T: Future<Output = Result<<MsgType as RpcMessage>::Item, <MsgType as RpcMessage>::Error>> + 'static
{}
