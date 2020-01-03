//! Provider part of Activity API
use crate::Result;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

rest_interface! {
    /// Bindings for Provider part of the Activity API.
    impl ProviderApiClient {

        /// Fetch Requestor command events.
        pub async fn get_activity_events(
            &self,
            #[query] timeout: Option<i32>
        ) -> Result<Vec<ProviderEvent>> {
            let response = get("activity/events/").send().json();

            response
        }

        /// Pass activity state (which may include error details)
        pub async fn set_activity_state(
            &self,
            state: ActivityState,
            #[path] activity_id: &str
        ) -> Result<()> {
            let response = put("activity/{activity_id}/state/").send_json( &state ).body();

            { response.map(|_| ()) }
        }

        /// Pass current activity usage (which may include error details)
        pub async fn set_activity_usage(
            &self,
            usage: ActivityUsage,
            #[path] activity_id: &str
        ) -> Result<()> {
            let response = put("activity/{activity_id}/usage/").send_json( &usage ).body();

            { response.map(|_| ()) }
        }
    }
}
