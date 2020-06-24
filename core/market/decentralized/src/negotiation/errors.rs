use thiserror::Error;

use crate::db::dao::TakeEventsError;
use crate::db::models::{SubscriptionId, SubscriptionParseError};
use ya_persistence::executor::Error as DbError;

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {}

#[derive(Error, Debug)]
pub enum QueryEventsError {
    #[error("Timeout waiting for events for subscription [{0}].")]
    Timeout(SubscriptionId),
    #[error("Subscription id [{0}] was already unsubscribed")]
    Unsubscribed(SubscriptionId),
    #[error("Invalid subscription id. {0}")]
    InvalidSubscriptionId(#[from] SubscriptionParseError),
    #[error("Failed to get events from database. Error: {0}.")]
    FailedGetEvents(#[from] TakeEventsError),
    #[error("Invalid maxEvents '{0}', should be greater from 0.")]
    InvalidMaxEvents(i32),
    #[error("Can't query events. Error: {0}.")]
    InternalError(String),
}

#[derive(Error, Debug)]
pub enum ProposalError {}
