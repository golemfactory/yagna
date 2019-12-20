//! Provider part of Market API
use crate::Result;
//use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};
use crate::web::{QueryParamsBuilder, WebClient};
use std::sync::Arc;
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

/// Bindings for Provider part of the Market API.
pub struct ProviderApi {
    client: Arc<WebClient>,
}

impl ProviderApi {
    pub fn new(client: Arc<WebClient>) -> Self {
        Self { client }
    }

    /// Publish Provider’s service capabilities (`Offer`) on the market to declare an
    /// interest in Demands meeting specified criteria.
    pub async fn subscribe(&self, offer: &Offer) -> Result<String> {
        Ok(String::from_utf8(
            self.client
                .post("offers/")
                .send_json(&offer)
                .body()
                .await?
                .to_vec(),
        )?)
    }

    /// Stop subscription by invalidating a previously published Offer.
    pub async fn unsubscribe(&self, subscription_id: &str) -> Result<String> {
        let url = url_format!("offers/{subscription_id}/", subscription_id);
        match self.client.delete(&url).send().json().await {
            Err(crate::Error::HttpStatusCode(awc::http::StatusCode::NO_CONTENT)) => Ok("".into()),
            x => x
        }
    }

    /// Get events which have arrived from the market in response to the Offer
    /// published by the Provider via  [`subscribe`](#method.subscribe).
    /// Returns collection of at most `max_events` `ProviderEvents` or times out.
    #[rustfmt::skip]
    pub async fn collect(
        &self,
        subscription_id: &str,
        timeout: Option<i32>,
        maxEvents: Option<i32>, // TODO: max_events
    ) -> Result<Vec<ProviderEvent>> {
        let url = url_format!(
            "offers/{subscription_id}/events",
            subscription_id,
            #[query] timeout,
            #[query] maxEvents
        );
        self.client.get(&url).send().json().await
    }

    /// Sends a bespoke Offer in response to specific Demand.
    pub async fn create_proposal(
        &self,
        proposal: &Proposal,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<String> {
        let url = url_format!(
            "offers/{subscription_id}/proposals/{proposal_id}/offer/",
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
            "offers/{subscription_id}/proposals/{proposal_id}/",
            subscription_id,
            proposal_id
        );
        self.client.get(&url).send().json().await
    }

    /// Rejects a bespoke Offer.
    pub async fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<String> {
        let url = url_format!(
            "offers/{subscription_id}/proposals/{proposal_id}/",
            subscription_id,
            proposal_id
        );
        self.client.delete(&url).send().json().await
    }

    /// Approves the Agreement received from the Requestor.
    /// Mutually exclusive with [`reject_agreement`](#method.reject_agreement).
    pub async fn approve_agreement(&self, agreement_id: &str) -> Result<String> {
        let url = url_format!("agreements/{agreement_id}/approve/", agreement_id);
        Ok(String::from_utf8(self.client.post(&url).send().body().await?.to_vec())?)
    }

    /// Rejects the Agreement received from the Requestor.
    /// Mutually exclusive with [`approve_agreement`](#method.approve_agreement).
    pub async fn reject_agreement(&self, agreement_id: &str) -> Result<()> {
        let url = url_format!("agreements/{agreement_id}/reject/", agreement_id);
        self.client.post(&url).send().json().await
    }
}
