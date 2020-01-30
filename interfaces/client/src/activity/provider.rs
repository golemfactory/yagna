//! Provider part of Activity API
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

use crate::{web::WebClient, web::WebInterface, Error, Result};

pub struct ActivityProviderApi {
    client: WebClient,
}

impl WebInterface for ActivityProviderApi {
    const API_URL_ENV_VAR: &'static str = super::YAGNA_ACTIVITY_URL_ENV_VAR;
    const API_SUFFIX: &'static str = super::ACTIVITY_API;

    fn from(client: WebClient) -> Self {
        ActivityProviderApi { client }
    }
}

/// Bindings for Provider part of the Activity API.
impl ActivityProviderApi {
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
