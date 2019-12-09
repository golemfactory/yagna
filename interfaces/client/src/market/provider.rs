use awc::Client;
use futures::{
    future::{BoxFuture, LocalBoxFuture},
    Future,
};
use std::sync::Arc;

use super::ApiConfiguration;
use crate::Error;
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

pub struct ProviderApiClient {
    configuration: Arc<ApiConfiguration>,
}

impl ProviderApiClient {
    pub fn new(configuration: Arc<ApiConfiguration>) -> Self {
        ProviderApiClient { configuration }
    }
}

pub trait ProviderApi {
    /// Publish Provider’s service capabilities (Offer) on the market to declare an
    /// interest in Demands meeting specified criteria.
    fn subscribe(&self, offer: Offer) -> LocalBoxFuture<Result<String, Error>>;

    /// Stop subscription by invalidating a previously published Offer.
    fn unsubscribe(&self, subscription_id: &str) -> LocalBoxFuture<Result<(), Error>>;

    /// Get events which have arrived from the market in response to the Offer
    /// published by the Provider via  [subscribe](self::subscribe).
    /// Returns collection of [ProviderEvents](ProviderEvent) or timeout.
    fn collect(
        &self,
        subscription_id: &str,
        timeout: f32,
        max_events: i64,
    ) -> BoxFuture<dyn Future<Output = Result<Vec<ProviderEvent>, Error>>>;

    /// TODO doc
    fn create_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
        proposal: Proposal,
    ) -> BoxFuture<dyn Future<Output = Result<String, Error>>>;

    /// TODO doc
    fn get_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<AgreementProposal, Error>>>;

    /// TODO doc
    fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<(), Error>>>;

    /// Confirms the Agreement received from the Requestor.
    /// Mutually exclusive with [reject_agreement](self::reject_agreement).
    fn approve_agreement(
        &self,
        agreement_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<(), Error>>>;

    /// Rejects the Agreement received from the Requestor.
    /// Mutually exclusive with [approve_agreement](self::approve_agreement).
    fn reject_agreement(
        &self,
        agreement_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<(), Error>>>;
}

impl ProviderApi for ProviderApiClient {
    fn subscribe(&self, offer: Offer) -> LocalBoxFuture<Result<String, Error>> {
        Box::pin(async move {
            let vec = Client::default()
                .post(self.configuration.api_endpoint("offers"))
                .send_json(&offer)
                .await?
                .body()
                .await?
                .to_vec();
            String::from_utf8(vec).map_err(Error::InvalidString)
        })
    }

    fn unsubscribe(&self, subscription_id: &str) -> LocalBoxFuture<Result<(), Error>> {
        //        Box::pin(async {
        //            Client::default()
        //                .delete(self.configuration.api_endpoint(format!("/offers/{}", subscription_id))?)
        //                .send_json(&Offer::new(serde_json::json!({"zima":"już"}), "()".into()))
        //                .await
        //                .expect("Offers POST request failed")
        //        })
        unimplemented!()
    }

    fn collect(
        &self,
        subscription_id: &str,
        timeout: f32,
        max_events: i64,
    ) -> BoxFuture<dyn Future<Output = Result<Vec<ProviderEvent>, Error>>> {
        //            "/offers/{subscriptionId}/events",
        unimplemented!()
    }

    fn create_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
        proposal: Proposal,
    ) -> BoxFuture<dyn Future<Output = Result<String, Error>>> {
        //            "/offers/{subscriptionId}/proposals/{proposalId}/offer".to_string(),
        unimplemented!()
    }

    fn get_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<AgreementProposal, Error>>> {
        //            "/offers/{subscriptionId}/proposals/{proposalId}".to_string(),
        unimplemented!()
    }

    fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<(), Error>>> {
        //            "/offers/{subscriptionId}/proposals/{proposalId}".to_string(),
        unimplemented!()
    }

    fn approve_agreement(
        &self,
        agreement_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<(), Error>>> {
        //            "/agreements/{agreementId}/approve".to_string(),
        unimplemented!()
    }

    fn reject_agreement(
        &self,
        agreement_id: &str,
    ) -> BoxFuture<dyn Future<Output = Result<(), Error>>> {
        //            "/agreements/{agreementId}/reject".to_string(),
        unimplemented!()
    }
}
