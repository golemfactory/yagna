use futures::{
    channel::oneshot,
    future::{BoxFuture, Future},
};

pub trait Context: Clone + Send + Sync {}

pub trait Cmd<C>
where
    Self: Send + Sync,
    C: Context + 'static,
{
    type Response: Send + 'static;
    type Result: Send + Future<Output = Self::Response>;

    fn action(self, ctx: C) -> Self::Result;
}

pub trait HandleCmd<C>
where
    Self: Context + 'static,
    C: Cmd<Self> + 'static,
{
    type Result: Future<Output = C::Response>;

    fn handle(&mut self, cmd: C) -> Self::Result;
}

impl<T, C> HandleCmd<C> for T
where
    T: Context + 'static,
    C: Cmd<T> + 'static,
{
    type Result = BoxFuture<'static, C::Response>;

    fn handle(&mut self, cmd: C) -> Self::Result {
        let (tx, rx) = oneshot::channel();
        let ctx = self.clone();
        tokio::spawn(async move {
            let res = cmd.action(ctx).await;
            if let Err(_) = tx.send(res) {
                panic!("send should succeed")
            }
        });
        Box::pin(async move { rx.await.expect("send should succeed") })
    }
}
