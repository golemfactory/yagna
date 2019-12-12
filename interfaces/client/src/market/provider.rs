//! Provider part of Market API
use crate::Result;
//use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

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
            let response = get("offers/{subscription_id}/events/").send().json();

            response
        }

        /// Sends a bespoke Offer in response to specific Demand.
        pub async fn create_proposal(
            &self,
            proposal: Proposal,
            #[path] subscription_id: &str,
            #[path] proposal_id: &str
        ) -> Result<String> {
            let response = post("offers/{subscription_id}/proposals/{proposal_id}/offer/")
                .send_json( &proposal ).body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Fetches `AgreementProposal` from proposal id.
        pub async fn get_proposal(
            &self,
            #[path] subscription_id: &str,
            #[path] proposal_id: &str
        ) -> Result<AgreementProposal> {
            let response = get("offers/{subscription_id}/proposals/{proposal_id}/")
                .send().json();

            response
        }

        /// Rejects a bespoke Offer.
        pub async fn reject_proposal(
            &self,
            #[path] subscription_id: &str,
            #[path] proposal_id: &str
        ) -> Result<()> {
            let _response = delete("offers/{subscription_id}/proposals/{proposal_id}/")
                .send().body();

            { Ok(()) }
        }

        /// Approves the Agreement received from the Requestor.
        /// Mutually exclusive with [`reject_agreement`](#method.reject_agreement).
        pub async fn approve_agreement(
            &self,
            #[path] agreement_id: &str
        ) -> Result<()> {
            let _response = post("agreements/{agreement_id}/approve/").send().body();

            { Ok(()) }
        }

        /// Rejects the Agreement received from the Requestor.
        /// Mutually exclusive with [`approve_agreement`](#method.approve_agreement).
        pub async fn reject_agreement(
            &self,
            #[path] agreement_id: &str
        ) -> Result<()> {
            let _response = post("agreements/{agreement_id}/reject/").send().body();

            { Ok(()) }
        }
    }
}
