use crate::Result;
use ya_model::activity::{ActivityState, ExeScriptCommandState};

rest_interface! {
    /// Bindings for Requestor State part of the Activity API.
    impl RequestorStateApiClient {
        pub async fn get_running_command(
            &self,
            #[path] activity_id: &str
        ) -> Result<ExeScriptCommandState> {
            let response = get("activity/{activity_id}/command/").send().json();

            { response }
        }

        pub async fn get_state(
            &self,
            #[path] activity_id: &str
        ) -> Result<ActivityState> {
            let response = get("activity/{activity_id}/state/").send().json();

            { response }
        }

        pub async fn get_usage(
            &self,
            #[path] activity_id: &str
        ) -> Result<Vec<f64>> {
            let response = get("activity/{activity_id}/usage/").send().json();

            { response }
        }
    }
}
