use futures::stream::StreamExt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ya_client::model::NodeId;
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};
use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::{EventError, OwnerType, Proposal};
use crate::matcher::OfferError;
use crate::matcher::SubscriptionStore;
use crate::negotiation::notifier::{EventNotifier, NotifierError};
use crate::negotiation::QueryEventsError;
use crate::protocol::negotiation::errors::{CounterProposalError, RemoteProposalError};
use crate::protocol::negotiation::messages::{
    AgreementCancelled, AgreementReceived, InitialProposalReceived, ProposalReceived,
    ProposalRejected,
};
use crate::protocol::negotiation::provider::NegotiationApi;
use crate::{db::models::Offer as ModelOffer, SubscriptionId};
use ya_client::model::market::event::ProviderEvent;

/// Provider part of negotiation logic.
#[derive(Clone)]
pub struct ProviderBroker {
    db: DbExecutor,
    store: SubscriptionStore,
    api: NegotiationApi,
    notifier: EventNotifier,
}

impl ProviderBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
    ) -> Result<Arc<ProviderBroker>, NegotiationInitError> {
        let notifier = EventNotifier::new();

        let db1 = db.clone();
        let notifier1 = notifier.clone();
        let store1 = store.clone();

        let api = NegotiationApi::new(
            move |caller: String, msg: InitialProposalReceived| {
                on_initial_proposal(db1.clone(), store1.clone(), notifier1.clone(), caller, msg)
            },
            move |_caller: String, msg: ProposalReceived| async move { unimplemented!() },
            move |caller: String, msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementReceived| async move { unimplemented!() },
            move |caller: String, msg: AgreementCancelled| async move { unimplemented!() },
        );

        Ok(Arc::new(ProviderBroker {
            api,
            store,
            db,
            notifier,
        }))
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        Ok(self.api.bind_gsb(public_prefix, private_prefix).await?)
    }

    pub async fn subscribe_offer(&self, offer: &ModelOffer) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.notifier.stop_notifying(subscription_id).await;
        Ok(())
    }

    pub async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<ProviderEvent>, QueryEventsError> {
        let mut timeout = Duration::from_secs_f32(timeout.max(0.0));
        let stop_time = Instant::now() + timeout;
        let max_events = max_events.unwrap_or(i32::max_value());

        if max_events < 0 {
            Err(QueryEventsError::InvalidMaxEvents(max_events))?
        } else if max_events == 0 {
            return Ok(vec![]);
        }

        loop {
            let events = get_events_from_db(&self.db, subscription_id, max_events).await?;
            if events.len() > 0 {
                return Ok(events);
            }

            // Solves panic 'supplied instant is later than self'.
            if stop_time < Instant::now() {
                return Ok(vec![]);
            }
            timeout = stop_time - Instant::now();

            if let Err(error) = self
                .notifier
                .wait_for_event_with_timeout(subscription_id, timeout)
                .await
            {
                return match error {
                    NotifierError::Timeout(_) => Ok(vec![]),
                    NotifierError::ChannelClosed(_) => {
                        Err(QueryEventsError::InternalError(format!("{}", error)))
                    }
                    NotifierError::Unsubscribed(id) => Err(QueryEventsError::Unsubscribed(id)),
                };
            }
            // Ok result means, that event with required subscription id was added.
            // We can go to next loop to get this event from db. But still we aren't sure
            // that list won't be empty, because other query_events calls can wait for the same event.
        }
    }
}

async fn get_events_from_db(
    db: &DbExecutor,
    subscription_id: &SubscriptionId,
    max_events: i32,
) -> Result<Vec<ProviderEvent>, QueryEventsError> {
    let events = db
        .as_dao::<EventsDao>()
        .take_events(subscription_id, max_events, OwnerType::Provider)
        .await?;

    // Map model events to client RequestorEvent.
    let results = futures::stream::iter(events)
        .then(|event| event.into_client_provider_event(&db))
        .collect::<Vec<Result<ProviderEvent, EventError>>>()
        .await;

    // Filter errors. Can we do something better with errors, than logging them?
    Ok(results
        .into_iter()
        .inspect(|result| {
            if let Err(error) = result {
                log::warn!("Error converting event to client type: {}", error);
            }
        })
        .filter_map(|event| event.ok())
        .collect::<Vec<ProviderEvent>>())
}

async fn on_initial_proposal(
    db: DbExecutor,
    store: SubscriptionStore,
    notifier: EventNotifier,
    caller: String,
    msg: InitialProposalReceived,
) -> Result<(), CounterProposalError> {
    // Check subscription.
    let offer = match store.get_offer(&msg.offer_id).await {
        Err(e) => match e {
            OfferError::AlreadyUnsubscribed(id) => Err(RemoteProposalError::Unsubscribed(id))?,
            OfferError::Expired(id) => Err(RemoteProposalError::Expired(id))?,
            _ => Err(RemoteProposalError::Unexpected(e.to_string()))?,
        },
        Ok(offer) => offer,
    };

    // Add proposal to database together with Negotiation record.
    let owner_id =
        NodeId::from_str(&caller).map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;
    let demand = msg.into_demand(owner_id);
    let proposal = Proposal::new_provider_initial(demand, offer);
    let proposal = db
        .as_dao::<ProposalDao>()
        .save_initial_proposal(proposal)
        .await
        .map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;

    // Create Proposal Event and add it to queue (database).
    let subscription_id = proposal.negotiation.subscription_id.clone();
    db.as_dao::<EventsDao>()
        .add_proposal_event(proposal, OwnerType::Provider)
        .await
        .map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;

    // Send channel message to wake all query_events waiting for proposals.
    notifier.notify(&subscription_id).await;
    Ok(())
}
