use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::{proposal::State, Proposal};

use super::{error::ResolverError, SubscriptionStore};
use crate::db::models::{Demand, Offer, SubscriptionId};

#[derive(Debug)]
pub enum Subscription {
    Offer(SubscriptionId),
    Demand(SubscriptionId),
}

impl From<&Offer> for Subscription {
    fn from(o: &Offer) -> Self {
        Subscription::Offer(o.id.clone())
    }
}

impl From<&Demand> for Subscription {
    fn from(d: &Demand) -> Self {
        Subscription::Demand(d.id.clone())
    }
}

/// Resolves the match relation for the specific Offer-Demand pair.
#[derive(Clone)]
pub struct Resolver {
    pub(crate) store: SubscriptionStore,
    subscription_tx: UnboundedSender<Subscription>,
    proposal_tx: UnboundedSender<Proposal>,
}

impl Resolver {
    pub fn new(store: SubscriptionStore, proposal_tx: UnboundedSender<Proposal>) -> Self {
        let (subscription_tx, subscription_rx) = unbounded_channel::<Subscription>();

        let resolver = Resolver {
            store,
            subscription_tx,
            proposal_tx,
        };

        tokio::spawn({
            let resolver = resolver.clone();
            async move {
                resolver
                    .process_incoming_subscriptions(subscription_rx)
                    .await
            }
        });

        resolver
    }

    pub fn receive(&self, subscription: impl Into<Subscription>) -> Result<(), ResolverError> {
        Ok(self.subscription_tx.send(subscription.into())?)
    }

    // TODO: it is mocked; emits dummy Proposal upon every Subscription
    async fn process_incoming_subscriptions(
        &self,
        mut subscription_rx: UnboundedReceiver<Subscription>,
    ) {
        while let Some(new_subs) = subscription_rx.recv().await {
            log::debug!("processing incoming subscription {:?}", new_subs);
            // TODO: here we will use Store to get list of all active Offers or Demands
            // TODO: to be resolved against newcomer subscription
            let (proposal, id) = match new_subs {
                Subscription::Offer(id) => {
                    log::info!("TODO: resolve new Offer: {:?}", id);
                    let offer = self.store.get_offer(&id).await.unwrap();
                    let client_offer = offer.clone().into_client_offer().unwrap();
                    (
                        Proposal {
                            properties: client_offer.properties,
                            constraints: client_offer.constraints,
                            proposal_id: Some(id.to_string()), // TODO: generate new id
                            issuer_id: Some(offer.node_id.to_string()),
                            state: Some(State::Initial),
                            prev_proposal_id: None,
                        },
                        id,
                    )
                }
                Subscription::Demand(id) => {
                    log::info!("TODO: resolve new Demand: {:?}", id);
                    let demand = self.store.get_demand(&id).await.unwrap();
                    let client_demand = demand.clone().into_client_demand().unwrap();
                    (
                        Proposal {
                            properties: client_demand.properties,
                            constraints: client_demand.constraints,
                            proposal_id: Some(id.to_string()), // TODO: generate new id
                            issuer_id: Some(demand.node_id.to_string()),
                            state: Some(State::Initial),
                            prev_proposal_id: None,
                        },
                        id,
                    )
                }
            };
            // TODO: upon finding matching pair we will send a proposal

            if let Err(e) = self.proposal_tx.send(proposal) {
                // TODO: should we stop processing events / panic ?
                log::error!("Failed to emit proposal for subscription [{:?}]: {}", id, e)
            };
        }
    }
}

pub fn find_matches<'a>(
    offers: &'a Vec<Offer>,
    demands: &'a Vec<Demand>,
) -> Result<Vec<(&'a Offer, &'a Demand)>, ResolverError> {
    let mut res = vec![];
    for offer in offers {
        for demand in demands {
            if matches(offer, demand)? {
                res.push((offer, demand));
            }
        }
    }
    Ok(res)
}

fn matches(offers: &Offer, demands: &Demand) -> Result<bool, ResolverError> {
    // TODO
    Ok(true)
}

#[cfg(test)]
mod tests {
    use crate::matcher::resolver::matches;
    use crate::testing::mock_offer::{sample_demand, sample_offer};

    #[test]
    fn matches_empty() {
        assert!(matches(&sample_offer(), &sample_demand()).unwrap())
    }
}
