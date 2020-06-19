use thiserror::Error;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::Proposal;
use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;
use ya_service_api_web::middleware::Identity;

use crate::db::dao::*;
use crate::db::models::Demand as ModelDemand;
use crate::db::models::Offer as ModelOffer;
use crate::db::models::{SubscriptionId, SubscriptionValidationError};
use crate::protocol::{Discovery, DiscoveryInitError, Propagate, StopPropagateReason};
use crate::protocol::{OfferReceived, OfferUnsubscribed, RetrieveOffers};

#[derive(Error, Debug)]
pub enum DemandError {
    #[error("Failed to save Demand. Error: {0}.")]
    SaveDemandFailure(#[from] DbError),
    #[error("Failed to remove Demand [{1}]. Error: {0}.")]
    RemoveDemandFailure(DbError, SubscriptionId),
    #[error("Demand [{0}] doesn't exist.")]
    DemandNotExists(SubscriptionId),
}

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("Failed to save Offer. Error: {0}.")]
    SaveOfferFailure(#[from] DbError),
    #[error("Failed to remove Offer [{1}]. Error: {0}.")]
    UnsubscribeOfferFailure(UnsubscribeError, SubscriptionId),
    #[error("Offer [{0}] doesn't exist.")]
    OfferNotExists(SubscriptionId),
}

#[derive(Error, Debug)]
pub enum MatcherError {
    #[error(transparent)]
    DemandError(#[from] DemandError),
    #[error(transparent)]
    OfferError(#[from] OfferError),
    #[error(transparent)]
    SubscriptionValidation(#[from] SubscriptionValidationError),
    #[error("Unexpected Internal error: {0}.")]
    UnexpectedError(String),
}

#[derive(Error, Debug)]
pub enum MatcherInitError {
    #[error("Failed to initialize Discovery interface. Error: {0}.")]
    DiscoveryError(#[from] DiscoveryInitError),
    #[error("Failed to initialize database. Error: {0}.")]
    DatabaseError(#[from] DbError),
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
            move |_caller: String, msg: OfferReceived| {
                let database = database1.clone();
                on_offer_received(database, msg)
            },
            move |_caller: String, msg: OfferUnsubscribed| {
                let database = database2.clone();
                on_offer_unsubscribed(database, msg)
            },
            move |caller: String, msg: RetrieveOffers| async move {
                log::info!("Offers request received from: {}. Unimplemented.", caller);
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
        let _ = self
            .discovery
            .broadcast_offer(model_offer.clone())
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to broadcast offer [{1}]. Error: {0}.",
                    e,
                    model_offer.id,
                );
            });
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

    pub async fn unsubscribe_offer(
        &self,
        id: &Identity,
        subscription_id: &SubscriptionId,
    ) -> Result<(), MatcherError> {
        self.db
            .as_dao::<OfferDao>()
            .mark_offer_as_unsubscribed(&subscription_id)
            .await
            .map_err(|e| OfferError::UnsubscribeOfferFailure(e, subscription_id.clone()))?;

        // Broadcast only, if no Error occurred in previous step.
        // We ignore broadcast errors. Unsubscribing was finished successfully, so:
        // - We shouldn't bother agent with broadcasts
        // - Unsubscribe message probably will reach other markets, but later.
        let _ = self
            .discovery
            .broadcast_unsubscribe(id.identity.to_string(), subscription_id.clone())
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to broadcast unsubscribe offer [{1}]. Error: {0}.",
                    e,
                    subscription_id
                );
            });
        Ok(())
    }

    pub async fn unsubscribe_demand(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), MatcherError> {
        let removed = self
            .db
            .as_dao::<DemandDao>()
            .remove_demand(&subscription_id)
            .await
            .map_err(|e| DemandError::RemoveDemandFailure(e, subscription_id.clone()))?;

        if !removed {
            Err(DemandError::DemandNotExists(subscription_id.clone()))?;
        }
        Ok(())
    }

    // =========================================== //
    // Offer/Demand query
    // =========================================== //

    pub async fn get_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<Option<ModelOffer>, MatcherError> {
        Ok(self
            .db
            .as_dao::<OfferDao>()
            .get_offer(subscription_id)
            .await?)
    }

    pub async fn get_demand(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<Option<ModelDemand>, MatcherError> {
        Ok(self
            .db
            .as_dao::<DemandDao>()
            .get_demand(subscription_id)
            .await?)
    }
}

async fn on_offer_received(db: DbExecutor, msg: OfferReceived) -> Result<Propagate, ()> {
    async move {
        // We shouldn't propagate Offer, if we already have it in our database.
        // Note that when, we broadcast our Offer, it will reach us too, so it concerns
        // not only Offers from other nodes.
        //
        // Note: Infinite broadcasting is possible here, if we would just use get_offer function,
        // because it filters expired and unsubscribed Offers. Note what happens in such case:
        // We think that Offer doesn't exist, so we insert it to database every time it reaches us,
        // because get_offer will never return it. So we will never meet stop condition of broadcast!!
        // So be careful.
        let propagate = match db
            .as_dao::<OfferDao>()
            .get_offer_state(&msg.offer.id)
            .await?
        {
            OfferState::Active(_) => Propagate::False(StopPropagateReason::AlreadyExists),
            OfferState::Unsubscribed(_) => {
                Propagate::False(StopPropagateReason::AlreadyUnsubscribed)
            }
            OfferState::Expired(_) => Propagate::False(StopPropagateReason::Expired),
            OfferState::NotFound => Propagate::True,
        };

        if let Propagate::True = propagate {
            // Will reject Offer, if hash was computed incorrectly. In most cases
            // it could mean, that it could be some kind of attack.
            msg.offer.validate()?;

            let model_offer = msg.offer;
            db.as_dao::<OfferDao>()
                .create_offer(&model_offer)
                .await
                .map_err(OfferError::SaveOfferFailure)?;

            // TODO: Spawn matching with Demands.
        }
        Result::<_, MatcherError>::Ok(propagate)
    }
    .await
    .or_else(|e| {
        let reason = StopPropagateReason::Error(format!("{}", e));
        Ok(Propagate::False(reason))
    })
}

async fn on_offer_unsubscribed(db: DbExecutor, msg: OfferUnsubscribed) -> Result<Propagate, ()> {
    async move {
        db.as_dao::<OfferDao>()
            .mark_offer_as_unsubscribed(&msg.subscription_id)
            .await?;

        // We store only our Offers to keep history. Offers from other nodes
        // should be removed.
        // We are sure that we don't remove our Offer here, because we would got
        // `AlreadyUnsubscribed` error from `mark_offer_as_unsubscribed` above,
        // as it was already invoked before broadcast in `unsubscribe_offer`.
        // TODO: Maybe we should add check here, to be sure, that we don't remove own Offers.
        log::debug!("Removing unsubscribed Offer [{}].", &msg.subscription_id);
        let _ = db
            .as_dao::<OfferDao>()
            .remove_offer(&msg.subscription_id)
            .await
            .map_err(|_| {
                log::warn!(
                    "Failed to remove offer [{}] during unsubscribe.",
                    &msg.subscription_id
                );
            });
        Result::<_, UnsubscribeError>::Ok(Propagate::True)
    }
    .await
    .or_else(|e| {
        let reason = match e {
            UnsubscribeError::OfferExpired(_) => StopPropagateReason::Expired,
            UnsubscribeError::AlreadyUnsubscribed(_) => StopPropagateReason::AlreadyUnsubscribed,
            _ => StopPropagateReason::Error(e.to_string()),
        };
        Ok(Propagate::False(reason))
    })
}

// =========================================== //
// Errors From impls
// =========================================== //

impl From<ErrorMessage> for MatcherError {
    fn from(e: ErrorMessage) -> Self {
        MatcherError::UnexpectedError(e.to_string())
    }
}

impl From<DbError> for MatcherError {
    fn from(e: DbError) -> Self {
        MatcherError::UnexpectedError(e.to_string())
    }
}
