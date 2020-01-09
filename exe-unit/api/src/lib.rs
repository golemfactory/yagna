pub mod command;
pub mod golem;

pub mod prelude {
    pub use super::command::{from_json::CommandFromJson, many::StreamResponse, Dispatcher};
    pub use super::Error;
}

type Result<T> = std::result::Result<T, Error>;

use serde::{
    de::{Deserialize, Deserializer, Visitor},
    Serialize,
};
use std::fmt;

#[derive(thiserror::Error, Debug, Serialize)]
pub enum Error {
    #[error("this value was deserialized")]
    Deserialized,
    #[error("oneshot channel Sender prematurely dropped")]
    OneshotCanceled(
        #[serde(skip)]
        #[from]
        futures::channel::oneshot::Canceled,
    ),
    #[error("actix mailbox error occurred {0}")]
    MailboxError(
        #[serde(skip)]
        #[from]
        actix::prelude::MailboxError,
    ),
    #[error("expected a JSON array object; got {0}")]
    WrongJson(serde_json::Value),
    #[error("deserializing command failed with {0}")]
    SerdeJson(
        #[serde(skip)]
        #[from]
        serde_json::Error,
    ),
}

impl<'de> Deserialize<'de> for Error {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Vis;

        impl<'de> Visitor<'de> for Vis {
            type Value = Error;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("enum Error")
            }

            #[inline]
            fn visit_newtype_struct<D>(
                self,
                _deserializer: D,
            ) -> std::result::Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                Ok(Error::Deserialized)
            }
        }

        deserializer.deserialize_newtype_struct("Error", Vis)
    }
}
