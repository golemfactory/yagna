use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use super::{error::ResolverError, RawProposal, SubscriptionStore};
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
    proposal_tx: UnboundedSender<RawProposal>,
}

impl Resolver {
    pub fn new(store: SubscriptionStore, proposal_tx: UnboundedSender<RawProposal>) -> Self {
        let (subscription_tx, subscription_rx) = unbounded_channel::<Subscription>();

        let resolver = Resolver {
            store,
            subscription_tx,
            proposal_tx,
        };

        // TODO: simplify
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

    // TODO: it is mocked; emits dummy RawProposal upon every Subscription
    async fn process_incoming_subscriptions(
        &self,
        mut subscription_rx: UnboundedReceiver<Subscription>,
    ) {
        while let Some(subscription) = subscription_rx.recv().await {
            log::debug!("processing incoming subscription {:?}", subscription);
            match subscription {
                Subscription::Offer(id) => {
                    log::info!("TODO: resolve new Offer: {:?}", id);
                    // TODO: get rid of unwraps
                    let offer = self.store.get_offer(&id).await.unwrap();
                    let demands = self.store.get_all_demands().await.unwrap();

                    for demand in demands {
                        self.emit_if_matches(offer.clone(), demand);
                    }
                }
                Subscription::Demand(id) => {
                    log::info!("TODO: resolve new Demand: {:?}", id);
                    let demand = self.store.get_demand(&id).await.unwrap();
                    let offers = self.store.get_all_offers().await.unwrap();

                    for offer in offers {
                        self.emit_if_matches(offer, demand.clone());
                    }
                }
            }
        }
    }

    // TODO: return Result; stop unwrapping
    pub fn emit_if_matches(&self, offer: Offer, demand: Demand) {
        if !matches(&offer, &demand).unwrap() {
            return;
        }

        let offer_id = offer.id.clone();
        let demand_id = demand.id.clone();
        if let Err(e) = self.proposal_tx.send(RawProposal { offer, demand }) {
            // TODO: should we stop processing events / panic ?
            log::error!(
                "Failed to emit proposal [offer_id:{}, demand_id:{}]: {}",
                offer_id,
                demand_id,
                e
            )
        };
    }
}

fn matches(offer: &Offer, demand: &Demand) -> Result<bool, ResolverError> {
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
