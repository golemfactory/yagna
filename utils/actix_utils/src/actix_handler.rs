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
                Result<
                    <<$MessageType as Message>::Result as ResultTypeGetter>::OkType,
                    <<$MessageType as Message>::Result as ResultTypeGetter>::ErrorType,
                >,
            >;

            fn handle(
                &mut self,
                msg: $MessageType,
                ctx: &mut <Self as Actor>::Context,
            ) -> Self::Result {
                ActorResponse::reply(self.$ForwardFun(msg, ctx))
            }
        }
    };
} // forward_actix_handler
