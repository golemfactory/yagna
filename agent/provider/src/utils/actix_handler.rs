

/// Generates actix handler function, that forwards function call
/// to class member function ($ForwardFun). $ForwardFun should be async
/// function. $ActorType should be structure that wraps implementation
/// inside Rc<RefCell<>>. $ActorImpl is field inside $ActorType that should be used
/// as self for $ForwardFun.
#[macro_export]
macro_rules! gen_actix_handler_async {
    ($ActorType:ty, $MessageType:ty, $ForwardFun:tt, $ActorImpl:tt) => {
        impl Handler<$MessageType> for $ActorType {
            type Result = ActorResponse<Self, (), Error>;

            fn handle(&mut self, msg: $MessageType, ctx: &mut Context<Self>) -> Self::Result {
                let mut actor_impl = self.$ActorImpl.clone();
                ActorResponse::r#async(async move {
                    (*actor_impl).borrow_mut().$ForwardFun(msg).await
                }.into_actor(self))
            }
        }
    };
}   // gen_actix_handler_async

