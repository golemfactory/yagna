

/// Generates actix handler function
#[macro_export]
macro_rules! gen_actix_handler_async {
    ($ActorType:ty, $MessageType:ty, $ForwardFun:tt, $ActorImpl:tt) => {
        impl Handler<$MessageType> for $ActorType {
            type Result = ActorResponse<Self, (), Error>;

            fn handle(&mut self, msg: $MessageType, ctx: &mut Context<Self>) -> Self::Result {
                trace!("ProviderMarket UpdateMarket message.");

                let mut actor_impl = self.$ActorImpl.clone();
                ActorResponse::r#async(async move {
                    (*actor_impl).borrow_mut().$ForwardFun(msg).await
                }.into_actor(self))
            }
        }
    };
}   // gen_actix_handler_async

