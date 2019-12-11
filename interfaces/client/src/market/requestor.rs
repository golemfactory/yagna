use awc::Client;

use crate::Result;
use ya_model::market::{Demand, RequestorEvent};

rest_interface! {
    /// Bindings for Requestor part of the Market API.
    impl RequestorApi {

        /// Publish Requestor’s service capabilities (Demand) on the market to declare an
        /// interest in Offers meeting specified criteria.
        pub async fn subscribe(&self, demand: Demand) -> Result<String> {
            let response = post("/demands").send_json( &demand ).body();
            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Stop subscription by invalidating a previously published Demand.
        pub async fn unsubscribe(&self, #[path] subscription_id: &str) -> Result<String> {
            let response = delete("/demands/{subscription_id}").send().body();
            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Get events which have arrived from the market in response to the Demand
        /// published by the Requestor via  [`subscribe`](#method.subscribe).
        /// Returns collection of at most `max_events` `RequestorEvents` or times out.
        pub async fn collect(
            &self,
            #[path] subscription_id: &str,
            #[path] timeout: f32,
            #[path] max_events: i64
        ) -> Result<Vec<RequestorEvent>> {
            let response = get("/demands/{subscription_id}/events/?timeout={timeout}&maxEvents={max_events}")
                .send().json();
            { response }
        }
    }
}
