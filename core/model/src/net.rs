pub const BUS_ID: &str = "/net";

// TODO: replace with dedicated endpoint/service descriptor with enum for visibility
pub const PUBLIC_PREFIX: &str = "/public";

///
///
pub mod local {
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Serialize};
    use ya_service_bus::RpcMessage;

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

        pub fn set_id(&mut self, id: String) {
            self.id = Some(id)
        }
    }

    impl<M> SendBroadcastMessage<M> {
        pub fn topic(&self) -> &str {
            self.topic.as_ref()
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
        pub fn with_endpoint<M: BroadcastMessage>(endpoint: impl Into<String>) -> Self {
            let topic = M::TOPIC.to_owned();
            let endpoint = endpoint.into();
            Self { topic, endpoint }
        }

        pub fn topic(&self) -> &str {
            self.topic.as_ref()
        }

        pub fn endpoint(&self) -> &str {
            self.endpoint.as_ref()
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
}
