use crate::{
    command::{many::Commands, Dispatcher},
    Error,
};
use actix::{
    dev::{AsyncContext, ToEnvelope},
    prelude::*,
};
use bus::{actix_rpc, Handle, RpcEnvelope, RpcMessage};
use futures::prelude::*;
use futures::TryStreamExt;
use serde::{
    de::{self, DeserializeOwned, Deserializer, SeqAccess, Visitor},
    Deserialize, Serialize,
};
use std::{fmt, marker::PhantomData};

pub struct BusEntrypoint<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M>,
{
    service_id: String,
    handle: Option<Handle>,
    recipient: Addr<Dispatcher<Ctx>>,
    p1: PhantomData<M>,
    p2: PhantomData<R>,
}

impl<M, R, Ctx> Unpin for BusEntrypoint<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M>,
{
}

impl<M, R, Ctx> BusEntrypoint<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M>,
{
    pub fn new(service_id: &str, recipient: Addr<Dispatcher<Ctx>>) -> Self {
        Self {
            service_id: service_id.to_owned(),
            handle: None,
            recipient,
            p1: PhantomData,
            p2: PhantomData,
        }
    }
}

impl<M, R, Ctx> Actor for BusEntrypoint<M, R, Ctx>
where
    M: Message<Result = R> + Serialize + DeserializeOwned + Send + Sync + 'static + Unpin,
    R: Serialize + Send + Sync + 'static,
    Ctx: Actor + Handler<M> + Send + Sync + 'static,
    <Ctx as Actor>::Context: AsyncContext<Ctx> + ToEnvelope<Ctx, M>,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.handle = Some(actix_rpc::bind::<Execute<M, R, Ctx>>(
            &self.service_id,
            ctx.address().recipient(),
        ));
        log::info!("listening on {}", &self.service_id);
    }
}

#[derive(Debug, Serialize)]
pub struct Execute<M, R, Ctx>
where
    M: Serialize + DeserializeOwned + Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    cmds: Vec<M>,
    p1: PhantomData<R>,
    p2: PhantomData<Ctx>,
}

impl<'de, M, R, Ctx> Deserialize<'de> for Execute<M, R, Ctx>
where
    M: Serialize + DeserializeOwned + Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ExecuteVisitor<M, R, Ctx>
        where
            M: Serialize + DeserializeOwned + Message<Result = R>,
            R: 'static,
            Ctx: Actor + Handler<M> + 'static,
        {
            p1: PhantomData<M>,
            p2: PhantomData<R>,
            p3: PhantomData<Ctx>,
        }

        impl<'de, M, R, Ctx> Visitor<'de> for ExecuteVisitor<M, R, Ctx>
        where
            M: Serialize + DeserializeOwned + Message<Result = R>,
            R: 'static,
            Ctx: Actor + Handler<M> + 'static,
        {
            type Value = Execute<M, R, Ctx>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("struct Execute")
            }

            #[inline]
            fn visit_newtype_struct<D>(
                self,
                deserializer: D,
            ) -> std::result::Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                let cmds: Vec<M> = match <Vec<M> as Deserialize>::deserialize(deserializer) {
                    Ok(val) => val,
                    Err(err) => return Err(err),
                };
                Ok(Execute {
                    cmds,
                    p1: PhantomData,
                    p2: PhantomData,
                })
            }

            #[inline]
            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let cmds = match match SeqAccess::next_element::<Vec<M>>(&mut seq) {
                    Ok(val) => val,
                    Err(err) => return Err(err),
                } {
                    Some(value) => value,
                    None => {
                        return Err(de::Error::invalid_length(
                            0usize,
                            &"struct Execute with 1 element",
                        ));
                    }
                };
                Ok(Execute {
                    cmds,
                    p1: PhantomData,
                    p2: PhantomData,
                })
            }
        }

        deserializer.deserialize_newtype_struct(
            "execute",
            ExecuteVisitor {
                p1: PhantomData,
                p2: PhantomData,
                p3: PhantomData,
            },
        )
    }
}

impl<M, R, Ctx> RpcMessage for Execute<M, R, Ctx>
where
    M: Serialize + DeserializeOwned + Message<Result = R> + Send + Sync + 'static,
    R: Send + Sync + 'static,
    Ctx: Actor + Handler<M> + Send + Sync + 'static,
{
    const ID: &'static str = "execute";
    type Item = String; // TODO this probably should not be a String, but need to discuss with MF
    type Error = Error;
}

impl<M, R, Ctx> Handler<RpcEnvelope<Execute<M, R, Ctx>>> for BusEntrypoint<M, R, Ctx>
where
    M: Message<Result = R> + Serialize + DeserializeOwned + Send + Sync + 'static + Unpin,
    R: Serialize + Send + Sync + 'static,
    Ctx: Actor + Handler<M> + Send + Sync + 'static,
    <Ctx as Actor>::Context: AsyncContext<Ctx> + ToEnvelope<Ctx, M>,
{
    type Result = ActorResponse<Self, String, Error>;

    fn handle(
        &mut self,
        msg: RpcEnvelope<Execute<M, R, Ctx>>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let dispatcher = self.recipient.clone();
        ActorResponse::r#async(
            async move {
                let response_stream = dispatcher
                    .send(Commands::new(msg.into_inner().cmds))
                    .await?
                    .into_inner();
                let v: Result<Vec<_>, ()> = response_stream.try_collect().await;
                match serde_json::to_string(v.as_ref().unwrap()) {
                    Ok(res) => Ok(res),
                    Err(e) => Err(Error::from(e)),
                }
            }
                .into_actor(self),
        )
    }
}
