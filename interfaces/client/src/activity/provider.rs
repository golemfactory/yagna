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

        /// Fetch activity state (which may include error details)
        pub async fn get_activity_state(
            &self,
            #[path] activity_id: &str
        ) -> Result<ActivityState> {
            let response = get("activity/{activity_id}/state/").send().json();

            response
        }

        /// Fetch current activity usage (which may include error details)
        pub async fn get_activity_usage(
            &self,
            #[path] activity_id: &str
        ) -> Result<ActivityUsage> {
            let response = get("activity/{activity_id}/usage/").send().json();

            response
        }
    }
}
