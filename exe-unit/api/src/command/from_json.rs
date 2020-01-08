use crate::{
    command::{many::StreamResponse, one::Command, Dispatcher},
    Error, Result,
};
use actix::{dev::ToEnvelope, prelude::*};
use futures::{
    prelude::*,
    stream::{self, Stream},
};
use serde::de::DeserializeOwned;
use std::marker::PhantomData;

pub struct CommandFromJson<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M>,
{
    json: String,
    p1: PhantomData<M>,
    p2: PhantomData<R>,
    p3: PhantomData<Ctx>,
}

impl<M, R, Ctx> CommandFromJson<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M>,
{
    pub fn new(json: String) -> Self {
        Self {
            json,
            p1: PhantomData,
            p2: PhantomData,
            p3: PhantomData,
        }
    }
}

impl<M, R, Ctx> Message for CommandFromJson<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M>,
{
    type Result = StreamResponse<Result<R>, ()>;
}

impl<M, R, Ctx> Handler<CommandFromJson<M, R, Ctx>> for Dispatcher<Ctx>
where
    M: Message<Result = R> + DeserializeOwned + Send + 'static,
    R: Send + 'static,
    Ctx: Actor + Handler<M> + Send + 'static,
    <Ctx as Actor>::Context: AsyncContext<Ctx> + ToEnvelope<Ctx, M>,
{
    type Result = StreamResponse<Result<R>, ()>;

    fn handle(&mut self, msg: CommandFromJson<M, R, Ctx>, ctx: &mut Self::Context) -> Self::Result {
        // we expect input to be a JSON Array even if it consists of one element
        match serde_json::from_str(&msg.json).map_err(Into::into) {
            // OK, we got a JSON array, now interpret Values as strongly typed Cmd
            // in case we cannot interpret any command, throw an error but do not
            // interrupt the flow
            Ok(serde_json::Value::Array(cmds)) => {
                let dispatcher = ctx.address();
                let cmds: Vec<_> = cmds
                    .into_iter()
                    .map(|cmd| (dispatcher.clone(), cmd))
                    .collect();
                StreamResponse::new(stream::iter_ok(cmds).and_then(|(dispatcher, cmd)| {
                    let cmd: Result<M> = serde_json::from_value(cmd).map_err(Into::into);
                    match cmd {
                        Ok(cmd) => {
                            future::Either::A(dispatcher.send(Command::new(cmd)).then(|res| {
                                async move {
                                    match res {
                                        Err(e) => future::ok(Err(Error::from(e))),
                                        Ok(res) => future::ok(res),
                                    }
                                }
                            }))
                        }
                        Err(e) => future::Either::B(future::ok(Err(e))),
                    }
                }))
            }
            Ok(value) => StreamResponse::new(stream::once(Ok(Err(Error::WrongJson(value))))),
            Err(e) => StreamResponse::new(stream::once(Ok(Err(e)))),
        }
    }
}
