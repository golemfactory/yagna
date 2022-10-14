use chrono::{NaiveDateTime, Utc};
use std::str::FromStr;
use std::sync::Arc;

use ya_client::model::market::Proposal;

use crate::db::model::{EventType, Owner, ProposalId, SubscriptionId};
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
            Owner::Requestor,
        ),
        timestamp,
        reason: None,
    }
}

const QUERY_EVENTS_TIMEOUT: f32 = 5.0;

pub mod requestor {
    use super::*;
    use ya_client::model::market::event::RequestorEvent;
    use ya_client::model::market::{AgreementEventType, AgreementOperationEvent as AgreementEvent};

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
            AgreementEventType::AgreementApprovedEvent { .. } => Ok(events[0].clone().agreement_id),
            _ => panic!("Expected AgreementEventType::AgreementApprovedEvent"),
        }
    }
}

pub mod provider {
    use super::*;
    use ya_client::model::market::event::ProviderEvent;
    use ya_client::model::market::Agreement;

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
