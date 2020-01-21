

#[macro_export]
macro_rules! gen_actix_handler_async {
    ($ActorType:ty, $MessageType:ty, $ForwardFun:tt) => {
        impl Handler<$MessageType> for $ActorType {
            type Result = ActorResponse<Self, (), Error>;

            fn handle(&mut self, msg: $MessageType, ctx: &mut Context<Self>) -> Self::Result {
                trace!("ProviderMarket UpdateMarket message.");

                let mut market_provider = self.market.clone();
                ActorResponse::r#async(async move {
                    (*market_provider).borrow_mut().$ForwardFun(msg).await
                }.into_actor(self))
            }
        }
    };
}   // gen_actix_handler_async

