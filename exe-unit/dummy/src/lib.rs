pub mod state;
pub mod worker;

type Result<T> = std::result::Result<T, Error>;

use serde::Serialize;

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
    #[error("received tokio timer error {0}")]
    TokioTimer(
        #[serde(skip)]
        #[from]
        tokio::timer::Error,
    ),
    #[error("invalid transition {transition:?} from state {state:?}")]
    InvalidTransition {
        transition: state::Transition,
        state: state::State,
    },
}
