//! Requestor part of Market API
use crate::Result;
use ya_model::market::{Agreement, AgreementProposal, Demand, Proposal, RequestorEvent};

rest_interface! {
    /// Bindings for Requestor part of the Market API.
    impl RequestorApi {

        /// Publish Requestor’s service capabilities (`Demand`) on the market to declare an
        /// interest in Offers meeting specified criteria.
        pub async fn subscribe(
            &self,
            demand: Demand
        ) -> Result<String> {
            let response = post("demands/").send_json( &demand ).body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Stop subscription by invalidating a previously published Demand.
        pub async fn unsubscribe(
            &self,
            #[path] subscription_id: &str
        ) -> Result<String> {
            let response = delete("demands/{subscription_id}/").send().body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Get events which have arrived from the market in response to the Demand
        /// published by the Requestor via  [`subscribe`](#method.subscribe).
        /// Returns collection of at most `max_events` `RequestorEvents` or times out.
        pub async fn collect(
            &self,
            #[path] subscription_id: &str,
            #[query] timeout: Option<i32>,
            #[query] maxEvents: Option<i32>  // TODO: max_events
        ) -> Result<Vec<RequestorEvent>> {
            let response = get("demands/{subscription_id}/events/")
                .send().json();

            response
        }

        /// Sends a bespoke Demand in response to specific Offer.
        pub async fn create_proposal(
            &self,
            proposal: Proposal,
            #[path] subscription_id: &str,
            #[path] proposal_id: &str
        ) -> Result<String> {
            let response = post("demands/{subscription_id}/proposals/{proposal_id}/demand/")
                .send_json( &proposal ).body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Fetches `AgreementProposal` from proposal id.
        pub async fn get_proposal(
            &self,
            #[path] subscription_id: &str,
            #[path] proposal_id: &str
        ) -> Result<AgreementProposal> {
            let response = get("demands/{subscription_id}/proposals/{proposal_id}/")
                .send().json();

            response
        }

        /// Rejects a bespoke Demand.
        pub async fn reject_proposal(
            &self,
            #[path] subscription_id: &str,
            #[path] proposal_id: &str
        ) -> Result<()> {
            let response = delete("demands/{subscription_id}/proposals/{proposal_id}/")
                .send().body();

            { response.map(|_| ()) }
        }

        /// Creates new Agreement from Proposal and sends to the Provider.
        /// Initiates the Agreement handshake phase.
        pub async fn create_agreement(
            &self,
            agreement: Agreement
        ) -> Result<()> {
            let response = post("agreements/").send_json( &agreement ).body();

            { response.map(|_| ()) }
        }

        // TODO: seems not needed -- wait_for_approval is enough
        /// Finally confirms the Agreement approved by the Provider.
        /// Mutually exclusive with [`cancel_agreement`](#method.cancel_agreement).
        pub async fn confirm_agreement(
            &self,
            #[path] agreement_id: &str
        ) -> Result<()> {
            let response = post("agreements/{agreement_id}/confirm/").send().body();

            { response.map(|_| ()) }
        }

        /// Waits for the response from Provider after an Agreement has been sent,
        /// expecting corresponding ApproveAgreement message.
        /// Mutually exclusive with [`cancel_agreement`](#method.cancel_agreement).
        pub async fn wait_for_approval(
            &self,
            #[path] agreement_id: &str
        ) -> Result<()> {
            let response = post("agreements/{agreement_id}/wait/").send().body();

            { response.map(|_| ()) }
        }

        /// Cancels the Agreement while still in the Proposed state.
        /// Mutually exclusive with [`confirm_agreement`](#method.confirm_agreement)
        /// and [`wait_for_approval`](#method.wait_for_approval).
        pub async fn cancel_agreement(
            &self,
            #[path] agreement_id: &str
        ) -> Result<()> {
            let response = delete("agreements/{agreement_id}/").send().body();

            { response.map(|_| ()) }
        }

    }
}
