pub const BUS_ID: &str = "/net";

// TODO: replace with dedicated endpoint/service descriptor with enum for visibility
pub const PUBLIC_PREFIX: &str = "/public";

///
///
pub mod local {
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Serialize};
    use std::future::Future;
    use ya_service_bus::{typed as bus, Handle, RpcEndpoint, RpcMessage};

    pub const BUS_ID: &str = "/local/net";

    pub trait BroadcastMessage: Serialize + DeserializeOwned {
        const TOPIC: &'static str;
    }

    #[derive(Serialize, Deserialize)]
    pub struct SendBroadcastMessage<M> {
        id: Option<String>,
        topic: String,
        body: M,
    }

    impl<M: BroadcastMessage> SendBroadcastMessage<M> {
        pub fn new(body: M) -> Self {
            let id = None;
            let topic = M::TOPIC.to_owned();
            Self { id, topic, body }
        }

        pub fn body(&self) -> &M {
            &self.body
        }
    }

    impl<M> SendBroadcastMessage<M> {
        pub fn topic(&self) -> &str {
            self.topic.as_ref()
        }

        pub fn set_id(&mut self, id: String) {
            self.id = Some(id)
        }
    }

    impl<M: Send + Sync + Serialize + DeserializeOwned + 'static> RpcMessage
        for SendBroadcastMessage<M>
    {
        const ID: &'static str = "SendBroadcastMessage";
        type Item = ();
        type Error = ();
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Subscribe {
        topic: String,
        endpoint: String,
    }

    impl Subscribe {
        pub fn topic(&self) -> &str {
            self.topic.as_ref()
        }

        pub fn endpoint(&self) -> &str {
            self.endpoint.as_ref()
        }
    }

    pub trait ToEndpoint<M: BroadcastMessage> {
        fn into_subscribe_msg(endpoint: impl Into<String>) -> Subscribe;
    }

    impl<M: BroadcastMessage> ToEndpoint<M> for M {
        fn into_subscribe_msg(endpoint: impl Into<String>) -> Subscribe {
            let topic = M::TOPIC.to_owned();
            let endpoint = endpoint.into();
            Subscribe { topic, endpoint }
        }
    }

    impl RpcMessage for Subscribe {
        const ID: &'static str = "Subscribe";
        type Item = u64;
        type Error = SubscribeError;
    }

    #[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum SubscribeError {
        #[error("{0}")]
        RuntimeException(String),
    }

    #[derive(thiserror::Error, Debug)]
    pub enum BindBroadcastError {
        #[error(transparent)]
        SubscribeError(#[from] SubscribeError),
        #[error(transparent)]
        GsbError(#[from] ya_service_bus::error::Error),
    }

    pub async fn bind_broadcast_with_caller<MsgType, Output, F>(
        broadcast_address: &str,
        handler: F,
    ) -> Result<Handle, BindBroadcastError>
    where
        MsgType: BroadcastMessage + Send + Sync + 'static,
        Output: Future<
                Output = Result<
                    <SendBroadcastMessage<MsgType> as RpcMessage>::Item,
                    <SendBroadcastMessage<MsgType> as RpcMessage>::Error,
                >,
            > + 'static,
        F: FnMut(String, SendBroadcastMessage<MsgType>) -> Output + 'static,
    {
        log::debug!("Creating broadcast topic {}.", MsgType::TOPIC);

        // We send Subscribe message to local net, which will create Topic
        // and add broadcast_address to be endpoint, which will be called, when someone
        // will broadcast any Message related to this Topic.
        let subscribe_msg = MsgType::into_subscribe_msg(broadcast_address);
        bus::service(BUS_ID).send(subscribe_msg).await??;

        log::debug!(
            "Binding handler '{}' for broadcast topic {}.",
            broadcast_address,
            MsgType::TOPIC
        );

        // We created endpoint address above. Now we must add handler, which will
        // handle broadcasts forwarded to this address.
        Ok(bus::bind_with_caller(broadcast_address, handler))
    }
}
