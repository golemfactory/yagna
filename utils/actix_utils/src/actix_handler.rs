use actix::prelude::dev::ToEnvelope;
use actix::prelude::*;
use log::error;

/// Trait that allows to extract error type ok type from Result.
/// Could use std::ops::Try, but it is marked as unstable.
pub trait ResultTypeGetter {
    type ErrorType;
    type OkType;
}

impl<T, E> ResultTypeGetter for anyhow::Result<T, E> {
    type ErrorType = E;
    type OkType = T;
}

/// Generates actix handler function, that forwards function call
/// to class member function ($ForwardFun).
#[macro_export]
macro_rules! forward_actix_handler {
    ($ActorType:ty, $MessageType:ty, $ForwardFun:tt) => {
        impl Handler<$MessageType> for $ActorType {
            type Result = ActorResponse<
                Self,
                <<$MessageType as Message>::Result as ResultTypeGetter>::OkType,
                <<$MessageType as Message>::Result as ResultTypeGetter>::ErrorType,
            >;

            fn handle(&mut self, msg: $MessageType, _ctx: &mut Context<Self>) -> Self::Result {
                ActorResponse::reply(self.$ForwardFun(msg))
            }
        }
    };
} // gen_actix_handler_sync

// Sends message to other actor.
pub fn send_message<ActorType, MessageType>(actor: Addr<ActorType>, msg: MessageType)
where
    MessageType: Message + Send + 'static,
    MessageType::Result: Send,
    ActorType: Handler<MessageType>,
    ActorType::Context: ToEnvelope<ActorType, MessageType>,
{
    let future = async move {
        if let Err(error) = actor.send(msg).await {
            //TODO: We could print more information about error.
            error!("Error sending message: {}.", error);
        };
    };
    Arbiter::spawn(future);
}
