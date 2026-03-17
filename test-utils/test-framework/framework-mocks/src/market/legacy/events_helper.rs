use std::{str::FromStr, sync::Arc};

use ya_client::model::market::Proposal;
use ya_market::testing::ProposalId;

const QUERY_EVENTS_TIMEOUT: f32 = 5.0;

pub mod requestor {
    use super::*;
    use crate::market::legacy::MarketService;
    use ya_client::model::market::event::RequestorEvent;
    use ya_client::model::market::{AgreementEventType, AgreementOperationEvent as AgreementEvent};
    use ya_market::testing::SubscriptionId;

    pub fn expect_proposal(events: Vec<RequestorEvent>, stage: &str) -> anyhow::Result<Proposal> {
        assert_eq!(
            events.len(),
            1,
            "Requestor {}: Expected single proposal event: {:?}.",
            stage,
            events
        );

        Ok(match events[0].clone() {
            RequestorEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }

    pub async fn query_proposal(
        market: &Arc<MarketService>,
        demand_id: &SubscriptionId,
        stage: &str,
    ) -> anyhow::Result<Proposal> {
        let events = market
            .requestor_engine
            .query_events(demand_id, QUERY_EVENTS_TIMEOUT, Some(5))
            .await?;
        expect_proposal(events, stage)
    }

    pub fn expect_approve(events: Vec<AgreementEvent>, stage: &str) -> anyhow::Result<String> {
        assert_eq!(
            events.len(),
            1,
            "Requestor {}: Expected single agreement event: {:?}.",
            stage,
            events
        );

        match events[0].event_type {
            AgreementEventType::AgreementApprovedEvent => Ok(events[0].clone().agreement_id),
            _ => panic!("Expected AgreementEventType::AgreementApprovedEvent"),
        }
    }
}

pub mod provider {
    use super::*;
    use ya_client::model::market::event::ProviderEvent;
    use ya_client::model::market::Agreement;
    use ya_market::testing::SubscriptionId;
    use ya_market::MarketService;

    pub fn expect_proposal(events: Vec<ProviderEvent>, stage: &str) -> anyhow::Result<Proposal> {
        assert_eq!(
            events.len(),
            1,
            "Provider {}: Expected single proposal event: {:?}.",
            stage,
            events
        );

        Ok(match events[0].clone() {
            ProviderEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }

    pub fn expect_agreement(events: Vec<ProviderEvent>, stage: &str) -> anyhow::Result<Agreement> {
        assert_eq!(
            events.len(),
            1,
            "Provider {}: Expected single agreement event: {:?}.",
            stage,
            events
        );

        Ok(match events[0].clone() {
            ProviderEvent::AgreementEvent { agreement, .. } => agreement,
            _ => anyhow::bail!("Invalid event Type. AgreementEvent expected"),
        })
    }

    pub async fn query_proposal(
        market: &Arc<MarketService>,
        offer_id: &SubscriptionId,
        stage: &str,
    ) -> anyhow::Result<Proposal> {
        let events = market
            .provider_engine
            .query_events(offer_id, QUERY_EVENTS_TIMEOUT, Some(5))
            .await?;
        expect_proposal(events, stage)
    }
}

pub trait ClientProposalHelper {
    fn get_proposal_id(&self) -> anyhow::Result<ProposalId>;
}

impl ClientProposalHelper for Proposal {
    fn get_proposal_id(&self) -> anyhow::Result<ProposalId> {
        Ok(ProposalId::from_str(&self.proposal_id)?)
    }
}
