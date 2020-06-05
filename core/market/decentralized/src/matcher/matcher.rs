use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::{Demand, Offer, Proposal};
use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

use crate::db::dao::*;
use crate::db::models::Demand as ModelDemand;
use crate::db::models::Offer as ModelOffer;
use crate::db::models::{SubscriptionId, SubscriptionParseError};
use crate::db::*;
use crate::migrations;
use crate::protocol::{
    Discovery, DiscoveryError, DiscoveryInitError, Propagate, StopPropagateReason,
};
use crate::protocol::{OfferReceived, OfferUnsubscribed, RetrieveOffers};

#[derive(Error, Debug)]
pub enum DemandError {
    #[error("Failed to save Demand. Error: {0}.")]
    SaveDemandFailure(#[from] DbError),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    RemoveDemandFailure(DbError, String),
    #[error("Demand [{0}] doesn't exist.")]
    DemandNotExists(String),
}

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("Failed to save Offer. Error: {0}.")]
    SaveOfferFailure(#[from] DbError),
    #[error("Failed to remove Offer [{1}]. Error: {0}.")]
    RemoveOfferFailure(DbError, String),
    #[error("Offer [{0}] doesn't exist.")]
    OfferNotExists(String),
    #[error("Failed to broadcast offer [{1}]. Error: {0}.")]
    BroadcastOfferFailure(DiscoveryError, SubscriptionId),
    #[error("Failed to broadcast unsubscribe offer [{1}]. Error: {0}.")]
    BroadcastUnsubscribeOfferFailure(DiscoveryError, SubscriptionId),
}

#[derive(Error, Debug)]
pub enum MatcherError {
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    OfferError(#[from] OfferError),
    #[error("Internal error: {0}.")]
    InternalError(String),
}

#[derive(Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {0}.")]
    DiscoveryError(#[from] DiscoveryInitError),
    #[error("Failed to initialize database. Error: {0}.")]
    DatabaseError(#[from] DbError),
    #[error("Failed to migrate market database. Error: {0}.")]
    MigrationError(#[from] anyhow::Error),
}

/// Receivers for events, that can be emitted from Matcher.
pub struct EventsListeners {
    pub proposal_receiver: UnboundedReceiver<Proposal>,
}

/// Responsible for storing Offers and matching them with demands.
#[derive(Clone)]
pub struct Matcher {
    db: DbExecutor,
    discovery: Discovery,
    proposal_emitter: UnboundedSender<Proposal>,
}

impl Matcher {
    pub fn new(db: &DbExecutor) -> Result<(Matcher, EventsListeners), MatcherInitError> {
        // TODO: Implement Discovery callbacks.

        let database1 = db.clone();
        let database2 = db.clone();
        let discovery = Discovery::new(
            move |msg: OfferReceived| {
                let database = database1.clone();
                on_offer_received(database, msg)
            },
            move |msg: OfferUnsubscribed| {
                let database = database2.clone();
                on_offer_unsubscribed(database, msg)
            },
            move |msg: RetrieveOffers| async move {
                log::info!("Offers request received. Unimplemented.");
                Ok(vec![])
            },
        )?;
        let (emitter, receiver) = unbounded_channel::<Proposal>();

        let matcher = Matcher {
            db: db.clone(),
            discovery,
            proposal_emitter: emitter,
        };
        let listeners = EventsListeners {
            proposal_receiver: receiver,
        };

        Ok((matcher, listeners))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), MatcherInitError> {
        Ok(self
            .discovery
            .bind_gsb(public_prefix, private_prefix)
            .await?)
    }

    pub async fn add_offer(&self, offer: Offer) {
        unimplemented!();
    }

    // =========================================== //
    // Offer/Demand subscription
    // =========================================== //

    pub async fn subscribe_offer(&self, model_offer: &ModelOffer) -> Result<(), MatcherError> {
        self.db
            .as_dao::<OfferDao>()
            .create_offer(model_offer)
            .await
            .map_err(OfferError::SaveOfferFailure)?;

        // TODO: Run matching to find local matching demands. We shouldn't wait here.
        // TODO: Handle broadcast errors. Maybe we should retry if it failed.
        self.discovery
            .broadcast_offer(model_offer.into_client_offer()?)
            .await
            .map_err(|error| {
                OfferError::BroadcastOfferFailure(error, model_offer.id.clone())
            })?;
        Ok(())
    }

    pub async fn subscribe_demand(&self, model_demand: &ModelDemand) -> Result<(), MatcherError> {
        self.db
            .as_dao::<DemandDao>()
            .create_demand(model_demand)
            .await
            .map_err(DemandError::SaveDemandFailure)?;

        // TODO: Try to match demand with offers currently existing in database.
        //  We shouldn't await here on this.
        Ok(())
    }

    pub async fn unsubscribe_offer(&self, subscription_id: &str) -> Result<(), MatcherError> {
        let subscription_id = SubscriptionId::from_str(subscription_id)?;
        let removed = self
            .db
            .as_dao::<OfferDao>()
            .remove_offer(subscription_id.to_string())
            .await
            .map_err(|error| OfferError::RemoveOfferFailure(error, subscription_id.to_string()))?;

        if !removed {
            Err(OfferError::OfferNotExists(subscription_id.to_string()))?;
        } else {
            self.discovery
                .broadcast_unsubscribe(subscription_id.to_string())
                .await
                .map_err(|error| {
                    OfferError::BroadcastUnsubscribeOfferFailure(error, subscription_id)
                })?;
        }
        Ok(())
    }

    pub async fn unsubscribe_demand(&self, subscription_id: &str) -> Result<(), MatcherError> {
        let removed = self
            .db
            .as_dao::<DemandDao>()
            .remove_demand(subscription_id)
            .await
            .map_err(|error| {
                DemandError::RemoveDemandFailure(error, subscription_id.to_string())
            })?;

        if !removed {
            Err(DemandError::DemandNotExists(subscription_id.to_string()))?;
        }
        Ok(())
    }

    // =========================================== //
    // Offer/Demand query
    // =========================================== //

    pub async fn get_offer<Str: AsRef<str>>(
        &self,
        subscription_id: Str,
    ) -> Result<Option<Offer>, MatcherError> {
        let model_offer: Option<ModelOffer> = self
            .db
            .as_dao::<OfferDao>()
            .get_offer(subscription_id.as_ref())
            .await?;

        match model_offer {
            Some(model_offer) => Ok(Some(model_offer.into_client_offer()?)),
            None => Ok(None),
        }
    }

    pub async fn get_demand<Str: AsRef<str>>(
        &self,
        subscription_id: Str,
    ) -> Result<Option<Demand>, MatcherError> {
        let model_demand: Option<ModelDemand> = self
            .db
            .as_dao::<DemandDao>()
            .get_demand(subscription_id.as_ref())
            .await?;

        match model_demand {
            Some(model_demand) => Ok(Some(model_demand.into_client_offer()?)),
            None => Ok(None),
        }
    }
}

async fn on_offer_received(db: DbExecutor, msg: OfferReceived) -> Result<Propagate, ()> {
    async move {
        // We shouldn't propagate Offer, if we already have it in our database.
        // Note that when, we broadcast our Offer, it will reach us too, so it concerns
        // not only Offers from other nodes.
        if let Some(_) = db
            .as_dao::<OfferDao>()
            .get_offer(msg.offer.offer_id()?)
            .await?
        {
            return Ok(Propagate::False(StopPropagateReason::AlreadyExists));
        }

        let model_offer = ModelOffer::from(&msg.offer)?;
        db.as_dao::<OfferDao>()
            .create_offer(&model_offer)
            .await
            .map_err(OfferError::SaveOfferFailure)?;

        // TODO: Spawn matching with Demands.

        Result::<_, MatcherError>::Ok(Propagate::True)
    }
    .await
    .or_else(|error| {
        let reason = StopPropagateReason::Error(format!("{}", error));
        Ok(Propagate::False(reason))
    })
}

async fn on_offer_unsubscribed(db: DbExecutor, msg: OfferUnsubscribed) -> Result<Propagate, ()> {
    async move {
        // We should remove Offer if it wasn't ours. Ours Offers remain
        // in the database, but they are marked as unsubscribed.
        // TODO: Temporary version that always removes Offer.
        if let None = db
            .as_dao::<OfferDao>()
            .get_offer(&msg.subscription_id)
            .await?
        {
            return Ok(Propagate::False(StopPropagateReason::AlreadyUnsubscribed));
        }

        db.as_dao::<OfferDao>()
            .remove_offer(&msg.subscription_id)
            .await?;

        Result::<_, MatcherError>::Ok(Propagate::True)
    }
    .await
    .or_else(|error| {
        let reason = StopPropagateReason::Error(format!("{}", error));
        Ok(Propagate::False(reason))
    })
}

// =========================================== //
// Errors From impls
// =========================================== //

impl From<ErrorMessage> for MatcherError {
    fn from(e: ErrorMessage) -> Self {
        MatcherError::InternalError(e.to_string())
    }
}

impl From<SubscriptionParseError> for MatcherError {
    fn from(e: SubscriptionParseError) -> Self {
        MatcherError::InternalError(e.to_string())
    }
}

impl From<DbError> for MatcherError {
    fn from(e: DbError) -> Self {
        MatcherError::InternalError(e.to_string())
    }
}
