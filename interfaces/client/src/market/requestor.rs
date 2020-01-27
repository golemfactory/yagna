//! Requestor part of Market API
use std::sync::Arc;

use crate::{web::WebClient, Result};
use ya_model::market::{Agreement, AgreementProposal, Demand, Proposal, RequestorEvent};

/// Bindings for Requestor part of the Market API.
#[derive(Clone)]
pub struct RequestorApi {
    client: Arc<WebClient>,
}

impl RequestorApi {
    pub fn new(client: &Arc<WebClient>) -> Self {
        Self {
            client: client.clone(),
        }
    }
    /// Publish Requestor’s service capabilities (`Demand`) on the market to declare an
    /// interest in Offers meeting specified criteria.
    pub async fn subscribe(&self, demand: &Demand) -> Result<String> {
        self.client.post("demands/").send_json(&demand).json().await
    }

    /// Stop subscription by invalidating a previously published Demand.
    pub async fn unsubscribe(&self, subscription_id: &str) -> Result<String> {
        let url = url_format!("demands/{subscription_id}/", subscription_id);
        self.client.delete(&url).send().json().await
    }

    /// Get events which have arrived from the market in response to the Demand
    /// published by the Requestor via  [`subscribe`](#method.subscribe).
    /// Returns collection of at most `max_events` `RequestorEvents` or times out.
    #[rustfmt::skip]
    pub async fn collect(
        &self,
        subscription_id: &str,
        timeout: Option<i32>,
        #[allow(non_snake_case)]
        maxEvents: Option<i32>, // TODO: max_events
    ) -> Result<Vec<RequestorEvent>> {
        let url = url_format!(
            "demands/{subscription_id}/events",
            subscription_id,
            #[query] timeout,
            #[query] maxEvents
        );
        self.client.get(&url).send().json().await
    }

    /// Sends a bespoke Demand in response to specific Offer.
    pub async fn create_proposal(
        &self,
        proposal: &Proposal,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<String> {
        let url = url_format!(
            "demands/{subscription_id}/proposals/{proposal_id}/demand/",
            subscription_id,
            proposal_id
        );
        self.client.post(&url).send_json(&proposal).json().await
    }

    /// Fetches `AgreementProposal` from proposal id.
    pub async fn get_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<AgreementProposal> {
        let url = url_format!(
            "demands/{subscription_id}/proposals/{proposal_id}/",
            subscription_id,
            proposal_id
        );
        self.client.get(&url).send().json().await
    }

    /// Rejects a bespoke Demand.
    pub async fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<String> {
        let url = url_format!(
            "demands/{subscription_id}/proposals/{proposal_id}/",
            subscription_id,
            proposal_id
        );
        self.client.delete(&url).send().json().await
    }

    /// Creates new Agreement from Proposal and sends to the Provider.
    /// Initiates the Agreement handshake phase.
    pub async fn create_agreement(&self, agreement: &Agreement) -> Result<String> {
        self.client
            .post("agreements/")
            .send_json(&agreement)
            .json()
            .await
    }

    // TODO: seems not needed -- wait_for_approval is enough
    /// Finally confirms the Agreement approved by the Provider.
    /// Mutually exclusive with [`cancel_agreement`](#method.cancel_agreement).
    pub async fn confirm_agreement(&self, agreement_id: &str) -> Result<String> {
        let url = url_format!("agreements/{agreement_id}/confirm/", agreement_id);
        self.client.post(&url).send().json().await
    }

    /// Waits for the response from Provider after an Agreement has been sent,
    /// expecting corresponding ApproveAgreement message.
    /// Mutually exclusive with [`cancel_agreement`](#method.cancel_agreement).
    pub async fn wait_for_approval(&self, agreement_id: &str) -> Result<String> {
        let url = url_format!("agreements/{agreement_id}/wait/", agreement_id);
        self.client.post(&url).send().json().await
    }

    /// Cancels the Agreement while still in the Proposed state.
    /// Mutually exclusive with [`confirm_agreement`](#method.confirm_agreement)
    /// and [`wait_for_approval`](#method.wait_for_approval).
    pub async fn cancel_agreement(&self, agreement_id: &str) -> Result<()> {
        let url = url_format!("agreements/{agreement_id}/", agreement_id);
        self.client.delete(&url).send().json().await
    }
}
