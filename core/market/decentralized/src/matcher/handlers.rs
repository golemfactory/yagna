use ya_persistence::executor::DbExecutor;

use crate::db::dao::{OfferDao, OfferState, UnsubscribeError};
use crate::matcher::{MatcherError, OfferError};
use crate::protocol::{OfferReceived, OfferUnsubscribed, Propagate, StopPropagateReason};

pub(crate) async fn on_offer_received(
    db: DbExecutor, // TODO: use store
    _caller: String,
    msg: OfferReceived,
) -> Result<Propagate, ()> {
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
        Ok::<_, MatcherError>(propagate)
    }
    .await
    .or_else(|e| {
        let reason = StopPropagateReason::Error(format!("{}", e));
        Ok(Propagate::False(reason))
    })
}

pub(crate) async fn on_offer_unsubscribed(
    db: DbExecutor, // TODO: use store
    _caller: String,
    msg: OfferUnsubscribed,
) -> Result<Propagate, ()> {
    async move {
        db.as_dao::<OfferDao>()
            .mark_offer_as_unsubscribed(&msg.subscription_id)
            .await?;

        // We store only our Offers to keep history. Offers from other nodes
        // should be removed.
        // We are sure that we don't remove our Offer here, because we would got
        // `AlreadyUnsubscribed` error from `mark_offer_as_unsubscribed`,
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
        Ok(Propagate::True)
    }
    .await
    .or_else(|e| {
        let reason = match e {
            UnsubscribeError::OfferExpired => StopPropagateReason::Expired,
            UnsubscribeError::AlreadyUnsubscribed => StopPropagateReason::AlreadyUnsubscribed,
            _ => StopPropagateReason::Error(e.to_string()),
        };
        Ok(Propagate::False(reason))
    })
}
