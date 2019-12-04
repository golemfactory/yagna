pub mod core;

use futures::channel::oneshot;
use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExeUnitError {
    #[error("operation {0} in progress")]
    OpInProgress(String),
    #[error("oneshot")]
    Oneshot(#[from] oneshot::Canceled),
}

