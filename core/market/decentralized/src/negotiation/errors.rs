use crate::protocol::negotiation::errors::NegotiationApiInitError;
use thiserror::Error;

use crate::db::dao::TakeEventsError;
use crate::db::models::{SubscriptionId, SubscriptionParseError};

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {
    #[error("Failed to initialize Negotiation interface. Error: {0}.")]
    ApiInitError(#[from] NegotiationApiInitError),
}

#[derive(Error, Debug)]
pub enum QueryEventsError {
    #[error("Subscription [{0}] was already unsubscribed.")]
    Unsubscribed(SubscriptionId),
    #[error("Invalid subscription id. {0}")]
    InvalidSubscriptionId(#[from] SubscriptionParseError),
    #[error(transparent)]
    TakeEventsError(#[from] TakeEventsError),
    #[error("Invalid maxEvents '{0}', should be greater from 0.")]
    InvalidMaxEvents(i32),
    #[error("Can't query events. Error: {0}.")]
    InternalError(String),
}

#[derive(Error, Debug)]
pub enum ProposalError {}
