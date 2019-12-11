use crate::Result;
//use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};
use ya_model::market::{Offer, ProviderEvent};

rest_interface! {
    /// Bindings for Provider part of the Market API.
    impl ProviderApi {

        /// Publish Provider’s service capabilities (`Offer`) on the market to declare an
        /// interest in Demands meeting specified criteria.
        pub async fn subscribe(
            &self,
            offer: Offer
        ) -> Result<String> {
            let response = post("offers/").send_json( &offer ).body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Stop subscription by invalidating a previously published Offer.
        pub async fn unsubscribe(
            &self,
            #[path] subscription_id: &str
        ) -> Result<String> {
            let response = delete("offers/{subscription_id}/").send().body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Get events which have arrived from the market in response to the Offer
        /// published by the Provider via  [`subscribe`](#method.subscribe).
        /// Returns collection of at most `max_events` `ProviderEvents` or times out.
        pub async fn collect(
            &self,
            #[path] subscription_id: &str,
            #[query] timeout: Option<i32>,
            #[query] maxEvents: Option<i32>  // TODO: max_events
        ) -> Result<Vec<ProviderEvent>> {
            let response = get("offers/{subscription_id}/events/")
                .send().json();

            { response }
        }
    }
}
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
