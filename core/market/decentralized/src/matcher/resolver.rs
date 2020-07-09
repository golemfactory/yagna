use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_market_resolver::{match_demand_offer, Match};

use super::{error::ResolverError, RawProposal, SubscriptionStore};
use crate::db::models::{Demand, Offer, SubscriptionId};

#[derive(Debug)]
pub enum Subscription {
    Offer(SubscriptionId),
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

    pub fn receive(&self, subscription: impl Into<Subscription>) -> Result<(), ResolverError> {
        Ok(self.subscription_tx.send(subscription.into())?)
    }

    async fn process_incoming_subscriptions(
        self,
        mut subscription_rx: UnboundedReceiver<Subscription>,
    ) {
        while let Some(s) = subscription_rx.recv().await {
            log::debug!("Resolving incoming subscription {:?}", s);
            if let Err(e) = self.process_single_subscription(&s).await {
                log::warn!("Failed resolve subscription [{:?}]. Error: {}", s, e);
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
                let demands = self.store.get_demands_before(&offer).await?;

                for demand in demands {
                    self.emit_if_matches(offer.clone(), demand)?;
                }
            }
            Subscription::Demand(id) => {
                let demand = self.store.get_demand(id).await?;
                let offers = self.store.get_offers_before(&demand).await?;

                for offer in offers {
                    self.emit_if_matches(offer, demand.clone())?;
                }
            }
        }
        Ok(())
    }

    // TODO: return Result; stop unwrapping
    pub fn emit_if_matches(&self, offer: Offer, demand: Demand) -> Result<(), ResolverError> {
        if !matches(&offer, &demand)? {
            return Ok(());
        }

        let offer_id = offer.id.clone();
        let demand_id = demand.id.clone();
        Ok(self.proposal_tx.send(RawProposal { offer, demand })?)
    }
}

fn matches(offer: &Offer, demand: &Demand) -> Result<bool, ResolverError> {
    // TODO
    Ok(
        match match_demand_offer(
            &demand.properties,
            &demand.constraints,
            &offer.properties,
            &offer.constraints,
        )? {
            Match::Yes => true,
            _ => false,
        },
    )
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
