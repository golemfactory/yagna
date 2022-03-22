use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_market_resolver::{match_demand_offer, Match};

use super::{error::ResolverError, RawProposal, SubscriptionStore};
use crate::db::model::{Demand, Offer, SubscriptionId};

#[derive(Clone, Debug, derive_more::Display)]
pub enum Subscription {
    #[display(fmt = "Offer [{}]", _0)]
    Offer(SubscriptionId),
    #[display(fmt = "Demand [{}]", _0)]
    Demand(SubscriptionId),
}

impl From<&Offer> for Subscription {
    fn from(offer: &Offer) -> Self {
        Subscription::Offer(offer.id.clone())
    }
}

impl From<&Demand> for Subscription {
    fn from(demand: &Demand) -> Self {
        Subscription::Demand(demand.id.clone())
    }
}

/// Resolves the match relation for the specific Offer-Demand pair.
#[derive(Clone)]
pub struct Resolver {
    pub(crate) store: SubscriptionStore,
    subscription_tx: UnboundedSender<Subscription>,
    proposal_tx: UnboundedSender<RawProposal>,
}

impl Resolver {
    pub fn new(store: SubscriptionStore, proposal_tx: UnboundedSender<RawProposal>) -> Self {
        let (subscription_tx, subscription_rx) = unbounded_channel::<Subscription>();

        let myself = Resolver {
            store,
            subscription_tx,
            proposal_tx,
        };

        let resolver = myself.clone();
        tokio::spawn(resolver.process_incoming_subscriptions(subscription_rx));

        myself
    }

    pub fn receive(&self, subscription: impl Into<Subscription>) {
        let s = subscription.into();
        if let Err(e) = self.subscription_tx.send(s.clone()) {
            log::error!("Receiving incoming {:?} error: {:?}", s, e);
        };
    }

    async fn process_incoming_subscriptions(
        self,
        mut subscription_rx: UnboundedReceiver<Subscription>,
    ) {
        while let Some(s) = subscription_rx.recv().await {
            log::trace!("Resolving incoming {}", s);
            if let Err(e) = self.process_single_subscription(&s).await {
                log::warn!("Failed resolve [{}]. Error: {}", s, e);
            }
        }
    }

    async fn process_single_subscription(
        &self,
        subscription: &Subscription,
    ) -> Result<(), ResolverError> {
        match subscription {
            Subscription::Offer(id) => {
                let offer = self.store.get_offer(id).await?;
                self.store
                    .get_demands_before(offer.insertion_ts.unwrap())
                    .await?
                    .into_iter()
                    .filter(|demand| matches(&offer, &demand))
                    .for_each(|demand| self.emit_proposal(offer.clone(), demand));
            }
            Subscription::Demand(id) => {
                let demand = self.store.get_demand(id).await?;
                self.store
                    .get_offers_before(demand.insertion_ts.unwrap())
                    .await?
                    .into_iter()
                    .filter(|offer| matches(&offer, &demand))
                    .for_each(|offer| self.emit_proposal(offer, demand.clone()));
            }
        }
        Ok(())
    }

    pub fn emit_proposal(&self, offer: Offer, demand: Demand) {
        let offer_id = offer.id.clone();
        let demand_id = demand.id.clone();
        if let Err(e) = self.proposal_tx.send(RawProposal { offer, demand }) {
            log::warn!(
                "Emitting proposal for Offer [{}] and Demand [{}] error: {}",
                offer_id,
                demand_id,
                e
            );
        }
    }
}

fn matches(offer: &Offer, demand: &Demand) -> bool {
    if offer.node_id == demand.node_id {
        log::info!(
            "Rejecting Demand Offer pair from single identity. node_id: {}",
            offer.node_id
        );
        return false;
    }

    match match_demand_offer(
        &demand.properties,
        &demand.constraints,
        &offer.properties,
        &offer.constraints,
    ) {
        Ok(Match::Yes) => true,
        Err(e) => {
            log::warn!("Matching [{:?}] vs [{:?}] error: {}", offer, demand, e);
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::matcher::resolver::matches;
    use crate::testing::mock_offer::{sample_demand, sample_offer};

    #[test]
    fn matches_empty() {
        assert!(matches(&sample_offer(), &sample_demand()))
    }
}
