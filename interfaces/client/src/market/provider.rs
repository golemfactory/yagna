use awc::Client;
use futures::{compat::Future01CompatExt, Future};
use std::sync::Arc;

use super::ApiConfiguration;
use crate::Result;
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

//pub struct ProviderApi {
//    configuration: Arc<ApiConfiguration>,
//}
//
//impl ProviderApi {
//    pub fn new(configuration: Arc<ApiConfiguration>) -> Self {
//        ProviderApi { configuration }
//    }
//
//    /// Publish Provider’s service capabilities (Offer) on the market to declare an
//    /// interest in Demands meeting specified criteria.
//    pub async fn subscribe(&self, offer: Offer) -> Result<String> {
//        let result = Client::default()
//            .post(self.configuration.api_endpoint("offers"))
//            .send_json(&offer)
//            .compat()
//            .await?
//            .body()
//            .compat()
//            .await?;
//        let vec = result
//            .to_vec();
//        Ok(String::from_utf8(vec)?)
//    }
//}

rest_interface! {
    pub ProviderApi {
        #[field] configuration: Arc<ApiConfiguration>;

        #[REST(post, "offers", body)]
        fn subscribe(&self, offer: Offer) -> Result<String> {
            #[result] result;
            { Ok(String::from_utf8(result.to_vec())?) }
        }
    }
}


//    /// Stop subscription by invalidating a previously published Offer.
//    pub fn unsubscribe(&self, subscription_id: &str) -> impl Future<Output = Result<()>> {
//        //        Box::pin(async {
//        //            Client::default()
//        //                .delete(self.configuration.api_endpoint(format!("/offers/{}", subscription_id))?)
//        //                .send_json(&Offer::new(serde_json::json!({"zima":"już"}), "()".into()))
//        //                .await
//        //                .expect("Offers POST request failed")
//        //        })
//        async { unimplemented!() }
//    }
//
//    /// Get events which have arrived from the market in response to the Offer
//    /// published by the Provider via  [subscribe](self::subscribe).
//    /// Returns collection of [ProviderEvents](ProviderEvent) or timeout.
//    pub fn collect(
//        &self,
//        subscription_id: &str,
//        timeout: f32,
//        max_events: i64,
//    ) -> impl Future<Output = Result<Vec<ProviderEvent>>> {
//        //            "/offers/{subscriptionId}/events",
//        async { unimplemented!() }
//    }
//
//    /// TODO doc
//    pub fn create_proposal(
//        &self,
//        subscription_id: &str,
//        proposal_id: &str,
//        proposal: Proposal,
//    ) -> impl Future<Output = Result<String>> {
//        //            "/offers/{subscriptionId}/proposals/{proposalId}/offer"
//        async { unimplemented!() }
//    }
//
//    /// TODO doc
//    pub fn get_proposal(
//        &self,
//        subscription_id: &str,
//        proposal_id: &str,
//    ) -> impl Future<Output = Result<AgreementProposal>> {
//        //            "/offers/{subscriptionId}/proposals/{proposalId}"
//        async { unimplemented!() }
//    }
//
//    /// TODO doc
//    pub fn reject_proposal(
//        &self,
//        subscription_id: &str,
//        proposal_id: &str,
//    ) -> impl Future<Output = Result<()>> {
//        //            "/offers/{subscriptionId}/proposals/{proposalId}"
//        async { unimplemented!() }
//    }
//
//    /// Confirms the Agreement received from the Requestor.
//    /// Mutually exclusive with [reject_agreement](self::reject_agreement).
//    pub fn approve_agreement(&self, agreement_id: &str) -> impl Future<Output = Result<()>> {
//        //            "/agreements/{agreementId}/approve"
//        async { unimplemented!() }
//    }
//
//    /// Rejects the Agreement received from the Requestor.
//    /// Mutually exclusive with [approve_agreement](self::approve_agreement).
//    pub fn reject_agreement(&self, agreement_id: &str) -> impl Future<Output = Result<()>> {
//        //            "/agreements/{agreementId}/reject"
//        async { unimplemented!() }
//    }
//}
