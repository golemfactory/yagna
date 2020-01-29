//! Requestor state part of Activity API
use crate::web::WebClient;
use crate::Result;
use std::sync::Arc;
use ya_model::activity::{ActivityState, ExeScriptCommandState};

/// Bindings for Requestor State part of the Activity API.
pub struct RequestorStateApiClient {
    client: Arc<WebClient>,
}

impl RequestorStateApiClient {
    pub fn new(client: &Arc<WebClient>) -> Self {
        Self {
            client: client.clone(),
        }
    }

    /// Get running command for a specified Activity.
    pub async fn get_running_command(&self, activity_id: &str) -> Result<ExeScriptCommandState> {
        let uri = url_format!("activity/{activity_id}/command", activity_id);
        self.client.get(&uri).send().json().await
    }

    /// Get state of specified Activity.
    pub async fn get_state(&self, activity_id: &str) -> Result<ActivityState> {
        let uri = url_format!("activity/{activity_id}/state", activity_id);
        self.client.get(&uri).send().json().await
    }

    /// Get usage of specified Activity.
    pub async fn get_usage(&self, activity_id: &str) -> Result<Vec<f64>> {
        let uri = url_format!("activity/{activity_id}/usage", activity_id);
        self.client.get(&uri).send().json().await
    }
}
