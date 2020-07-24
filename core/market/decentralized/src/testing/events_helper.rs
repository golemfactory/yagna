use std::str::FromStr;
use std::sync::Arc;

use ya_client::model::market::Proposal;

use crate::db::model::ProposalId;
use crate::db::model::SubscriptionId;
use crate::MarketService;

pub mod requestor {
    use super::*;
    use ya_client::model::market::event::RequestorEvent;

    pub fn expect_proposal(events: Vec<RequestorEvent>) -> anyhow::Result<Proposal> {
        assert_ne!(events.len(), 0, "Expected one event. Found 0.");
        assert_eq!(events.len(), 1, "Expected only one event.");

        Ok(match events[0].clone() {
            RequestorEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }

    pub async fn query_proposal(
        market: &Arc<MarketService>,
        demand_id: &SubscriptionId,
    ) -> anyhow::Result<Proposal> {
        let events = market
            .requestor_engine
            .query_events(&demand_id, 2.2, Some(5))
            .await?;
        expect_proposal(events)
    }
}

pub mod provider {
    use super::*;
    use ya_client::model::market::event::ProviderEvent;

    pub fn expect_proposal(events: Vec<ProviderEvent>) -> anyhow::Result<Proposal> {
        assert_ne!(events.len(), 0, "Expected one event. Found 0.");
        assert_eq!(events.len(), 1, "Expected only one event.");

        Ok(match events[0].clone() {
            ProviderEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }

    pub async fn query_proposal(
        market: &Arc<MarketService>,
        offer_id: &SubscriptionId,
    ) -> anyhow::Result<Proposal> {
        let events = market
            .provider_engine
            .query_events(&offer_id, 2.2, Some(5))
            .await?;
        expect_proposal(events)
    }
}

pub trait ClientProposalHelper {
    fn get_proposal_id(&self) -> anyhow::Result<ProposalId>;
}

impl ClientProposalHelper for Proposal {
    fn get_proposal_id(&self) -> anyhow::Result<ProposalId> {
        Ok(ProposalId::from_str(self.proposal_id()?)?)
    }
}
