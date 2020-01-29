//! Provider part of Activity API
use crate::Result;
use awc::http::StatusCode;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

rest_interface! {
    /// Bindings for Provider part of the Activity API.
    impl ProviderApiClient {

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
/*
/// Fetch Requestor command events.
*/
impl ProviderApiClient {
    pub async fn get_activity_events(&self, timeout: Option<i32>) -> Result<Vec<ProviderEvent>> {
        let url = self.url(url_format!(
            "events",
            #[query]
            timeout
        ));
        let client: std::sync::Arc<_> = self.client.clone();

        let mut resp = client.awc.get(url.as_str()).send().await?;
        if resp.status() == StatusCode::REQUEST_TIMEOUT {
            Ok(Vec::new())
        } else {
            Ok(resp.json().await?)
        }
    }
}
