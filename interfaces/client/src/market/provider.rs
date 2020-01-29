//! Provider part of Market API

use crate::web::WebInterface;
use crate::{web::WebClient, Result};
use std::rc::Rc;
use std::str::FromStr;
use url::Url;
use ya_model::market::{Agreement, Offer, Proposal, ProviderEvent};

/// Bindings for Provider part of the Market API.
pub struct ProviderApi {
    client: WebClient,
}

impl ProviderApi {
    pub fn new(client: &WebClient) -> Self {
        Self {
            client: client.clone(),
        }
    }

    /// Publish Provider’s service capabilities (`Offer`) on the market to declare an
    /// interest in Demands meeting specified criteria.
    pub async fn subscribe_offer(&self, offer: &Offer) -> Result<String> {
        self.client.post("offers").send_json(&offer).json().await
    }

    /// Stop subscription by invalidating a previously published Offer.
    ///
    /// Stop receiving Proposals.
    /// **Note**: this will terminate all pending `collectDemands` calls on this subscription.
    /// This implies, that client code should not `unsubscribeOffer` before it has received
    /// all expected/useful inputs from `collectDemands`.
    pub async fn unsubscribe_offer(&self, subscription_id: &str) -> Result<String> {
        let url = url_format!("offers/{subscription_id}", subscription_id);
        self.client.delete(&url).send().json().await
    }

    /// Get events which have arrived from the market in response to the Offer
    /// published by the Provider via  [`subscribe`](#method.subscribe).
    /// Returns collection of at most `max_events` `ProviderEvents` or times out.
    #[rustfmt::skip]
    pub async fn collect_demands(
        &self,
        subscription_id: &str,
        timeout: Option<i32>,
        #[allow(non_snake_case)]
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

    /// Fetches Proposal (Demand) with given id.
    pub async fn get_proposal_demand(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<Proposal> {
        let url = url_format!(
            "offers/{subscription_id}/proposals/{proposal_id}",
            subscription_id,
            proposal_id
        );
        self.client.get(&url).send().json().await
    }

    /// Rejects Proposal (Demand).
    /// Effectively ends a Negotiation chain - it explicitly indicates that the sender
    /// will not create another counter-Proposal.
    pub async fn reject_proposal_demand(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<String> {
        let url = url_format!(
            "offers/{subscription_id}/proposals/{proposal_id}",
            subscription_id,
            proposal_id
        );
        self.client.delete(&url).send().json().await
    }

    /// Responds with a bespoke Offer to received Demand.
    /// Creates and sends a modified version of original Offer (a
    /// counter-proposal) adjusted to previously received Proposal (ie. Demand).
    /// Changes Proposal state to `Draft`. Returns created Proposal id.
    pub async fn create_proposal_offer(
        &self,
        proposal: &Proposal,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Result<String> {
        let url = url_format!(
            "offers/{subscription_id}/proposals/{proposal_id}/offer",
            subscription_id,
            proposal_id
        );
        self.client.post(&url).send_json(&proposal).json().await
    }

    /// Approves Agreement proposed by the Reqestor.
    ///
    /// This is a blocking operation.
    ///
    /// It returns one of the following options:
    ///
    /// * `Ok` - Indicates that the approved Agreement has been successfully
    /// delivered to the Requestor and acknowledged.
    /// - The Requestor side has been notified about the Provider’s commitment
    /// to the Agreement.
    /// - The Provider is now ready to accept a request to start an Activity
    /// as described in the negotiated agreement.
    /// - The Requestor’s corresponding ConfirmAgreement call returns Ok after
    /// the one on the Provider side.
    ///
    /// * `Cancelled` - Indicates that before delivering the approved Agreement,
    /// the Requestor has called `cancelAgreement`, thus invalidating the
    /// Agreement. The Provider may attempt to return to the Negotiation phase
    /// by sending a new Proposal.
    ///
    /// **Note**: It is expected from the Provider node implementation to “ring-fence”
    /// the resources required to fulfill the Agreement before the ApproveAgreement
    /// is sent. However, the resources should not be fully committed until `Ok`
    /// response is received from the `approveAgreement` call.
    ///
    ///
    /// **Note**: Mutually exclusive with `rejectAgreement`.
    pub async fn approve_agreement(&self, agreement_id: &str) -> Result<String> {
        let url = url_format!("agreements/{agreement_id}/approve", agreement_id);
        self.client.post(&url).send().json().await
    }

    /// Rejects Agreement proposed by the Requestor.
    ///
    /// The Requestor side is notified about the Provider’s decision to reject
    /// a negotiated agreement. This effectively stops the Agreement handshake.
    ///
    /// **Note**: Mutually exclusive with `approveAgreement`.
    pub async fn reject_agreement(&self, agreement_id: &str) -> Result<String> {
        let url = url_format!("agreements/{agreement_id}/reject", agreement_id);
        self.client.post(&url).send().json().await
    }

    /// Terminates approved Agreement.
    pub async fn terminate_agreement(&self, agreement_id: &str) -> Result<String> {
        let url = url_format!("agreements/{agreement_id}/terminate", agreement_id);
        self.client.post(&url).send().json().await
    }

    /// Fetches agreement with given agreement id.
    pub async fn get_agreement(&self, agreement_id: &str) -> Result<Agreement> {
        let url = url_format!("agreements/{agreement_id}", agreement_id);
        self.client.get(&url).send().json().await
    }
}

impl WebInterface for ProviderApi {
    fn rebase_service_url(base_url: Rc<Url>) -> Rc<Url> {
        if let Some(url) = std::env::var("YAGNA_MARKET_URL").ok() {
            return Rc::new(Url::from_str(&url).unwrap());
        }
        base_url.join("market-api/v1/").unwrap().into()
    }

    fn from_client(client: WebClient) -> Self {
        ProviderApi { client }
    }
}
