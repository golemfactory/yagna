//! Provider part of Activity API
use crate::web::WebClient;
use crate::{Error, Result};
use std::sync::Arc;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

pub struct ProviderApiClient {
    client: WebClient,
}

/// Bindings for Provider part of the Activity API.
impl ProviderApiClient {
    pub fn new(client: WebClient) -> Self {
        Self { client }
    }

    /// Fetch activity state (which may include error details)
    pub async fn get_activity_state(&self, activity_id: &str) -> Result<ActivityState> {
        let uri = url_format!("activity/{activity_id}/state", activity_id);
        self.client.get(&uri).send().json().await
    }

    /// Fetch current activity usage (which may include error details)
    pub async fn get_activity_usage(&self, activity_id: &str) -> Result<ActivityUsage> {
        let uri = url_format!("activity/{activity_id}/usage", activity_id);
        self.client.get(&uri).send().json().await
    }

    /// Fetch Requestor command events.
    pub async fn get_activity_events(&self, timeout: Option<i32>) -> Result<Vec<ProviderEvent>> {
        let url = url_format!(
            "events",
            #[query]
            timeout
        );

        match self.client.get(&url).send().json().await {
            Ok(v) => Ok(v),
            Err(e) => match e {
                Error::TimeoutError { .. } => Ok(Vec::new()),
                _ => Err(e),
            },
        }
    }
}
