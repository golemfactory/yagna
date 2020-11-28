use chrono::{NaiveDateTime, Utc};
use std::str::FromStr;
use std::sync::Arc;

use ya_client::model::market::Proposal;

use crate::db::model::{EventType, OwnerType, ProposalId, SubscriptionId};
use crate::db::schema::market_negotiation_event;
use crate::MarketService;

#[derive(Clone, Debug, Insertable, Queryable)]
#[table_name = "market_negotiation_event"]
pub struct TestMarketEvent {
    pub id: i32,
    pub subscription_id: SubscriptionId,
    pub timestamp: NaiveDateTime,
    pub event_type: EventType,
    pub artifact_id: ProposalId,
    pub reason: Option<String>,
}

pub fn generate_event(id: i32, timestamp: NaiveDateTime) -> TestMarketEvent {
    TestMarketEvent {
        id,
        subscription_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        event_type: EventType::ProviderNewProposal,
        artifact_id: ProposalId::generate_id(
                &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
                &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
                &Utc::now().naive_utc(),
                OwnerType::Requestor,
        ),
        timestamp,
        reason: None,
    }
}

pub mod requestor {
    use super::*;
    use ya_client::model::market::event::RequestorEvent;
    use ya_client::model::market::AgreementOperationEvent as AgreementEvent;

    pub fn expect_proposal(events: Vec<RequestorEvent>, i: u8) -> anyhow::Result<Proposal> {
        assert_eq!(events.len(), 1, "{}: Expected one event: {:?}.", i, events);

        Ok(match events[0].clone() {
            RequestorEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }

    pub async fn query_proposal(
        market: &Arc<MarketService>,
        demand_id: &SubscriptionId,
        i: u8,
    ) -> anyhow::Result<Proposal> {
        let events = market
            .requestor_engine
            .query_events(&demand_id, 2.2, Some(5))
            .await?;
        expect_proposal(events, i)
    }

    pub fn expect_approve(events: Vec<AgreementEvent>, i: u8) -> anyhow::Result<String> {
        assert_eq!(events.len(), 1, "{}: Expected one event: {:?}.", i, events);

        Ok(match events[0].clone() {
            AgreementEvent::AgreementApprovedEvent { agreement_id, .. } => agreement_id,
            _ => panic!("Expected AgreementEvent::AgreementApprovedEvent"),
        })
    }
}

pub mod provider {
    use super::*;
    use ya_client::model::market::event::ProviderEvent;

    pub fn expect_proposal(events: Vec<ProviderEvent>, i: u8) -> anyhow::Result<Proposal> {
        assert_eq!(events.len(), 1, "{}: Expected one event: {:?}.", i, events);

        Ok(match events[0].clone() {
            ProviderEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }

    pub async fn query_proposal(
        market: &Arc<MarketService>,
        offer_id: &SubscriptionId,
        i: u8,
    ) -> anyhow::Result<Proposal> {
        let events = market
            .provider_engine
            .query_events(&offer_id, 2.2, Some(5))
            .await?;
        expect_proposal(events, i)
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
