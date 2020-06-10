use thiserror::Error;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use ya_client::model::market::{Demand, Offer, Proposal};

use super::Matcher;

#[derive(Debug)]
pub enum Subscription {
    Offer(String),
    Demand(String),
}

/// Resolves the match relation for the specific Offer-Demand pair.
#[derive(Clone)]
pub struct Resolver {
    matcher: Matcher,
    subscription_tx: UnboundedSender<Subscription>,
}

#[derive(Error, Debug)]
pub enum ResolverInitError {
    #[error("Failed to start async resolver task: {0}.")]
    JoinError(#[from] tokio::task::JoinError),
}

#[derive(Error, Debug)]
pub enum ResolverError {
    #[error("Failed resolve matching relation for {0:?} and {1:?}.")]
    MatchingError(Offer, Demand),
    #[error("Failed to process incoming {0:?}")]
    SendError(#[from] tokio::sync::mpsc::error::SendError<Subscription>),
}

impl Resolver {
    pub fn new(matcher: Matcher) -> Result<Self, ResolverError> {
        let (subscription_tx, subscription_rx) = unbounded_channel::<Subscription>();

        let resolver = Resolver {
            matcher,
            subscription_tx,
        };

        tokio::spawn({
            let resolver = resolver.clone();
            async move {
                resolver
                    .process_incoming_subscriptions(subscription_rx)
                    .await
            }
        });

        Ok(resolver)
    }

    pub fn incoming_offer(&self, id: &str) -> Result<(), ResolverError> {
        Ok(self.subscription_tx.send(Subscription::Offer(id.into()))?)
    }

    pub fn incoming_demand(&self, id: &str) -> Result<(), ResolverError> {
        Ok(self.subscription_tx.send(Subscription::Demand(id.into()))?)
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

    async fn process_incoming_subscriptions(
        &self,
        mut subscription_rx: UnboundedReceiver<Subscription>,
    ) {
        while let Some(new_subs) = subscription_rx.recv().await {
            log::debug!("processing incoming subscription {:?}", new_subs);
            // TODO: here we will use Matcher to get list of all active Offers or Demands
            // TODO: to be resolved against newcomer subscription
            match new_subs {
                Subscription::Offer(id) => log::info!("TODO: resolve new Offer: {:?}", id),
                Subscription::Demand(id) => log::info!("TODO: resolve new Demand: {:?}", id),
            };
            // TODO: upon finding matching pair we will send a proposal
            let proposal = Proposal::new(
                serde_json::json!({"name": "dummy"}),
                "(&(name=dummy))".into(),
            );
            if let Err(e) = self.matcher.emit_proposal(proposal) {
                log::error!("Failed to emit proposal: {}", e)
            };
        }
    }
}

fn matches(offers: &Offer, demands: &Demand) -> Result<bool, ResolverError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    #[ignore]
    fn matches_empty() {
        let offer = Offer::new(Value::Null, "".into());
        let demand = Demand::new(Value::Null, "".into());
        assert!(matches(&offer, &demand).unwrap())
    }
}
