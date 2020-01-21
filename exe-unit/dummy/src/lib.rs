pub(crate) mod model;
pub mod state;
pub mod worker;

type Result<T> = std::result::Result<T, Error>;

use serde::Serialize;
use ya_model::activity::State;

#[derive(Debug, thiserror::Error, Serialize)]
pub enum Error {
    #[error("actix mailbox error {0}")]
    MailboxError(
        #[serde(skip)]
        #[from]
        actix::prelude::MailboxError,
    ),
    #[error("ExeUnit API error {0}")]
    ApiError(
        #[serde(skip)]
        #[from]
        api::prelude::Error,
    ),
    #[error("invalid transition {transition:?} from state {state:?}")]
    InvalidTransition {
        transition: state::Transition,
        state: State,
    },
    #[error("Service bus error {0}")]
    GsbError(String),
    #[error("Remote service error {0}")]
    RemoteServiceError(String),
}

impl From<ya_service_bus::Error> for Error {
    fn from(e: ya_service_bus::Error) -> Self {
        Error::GsbError(format!("{:?}", e))
    }
}
