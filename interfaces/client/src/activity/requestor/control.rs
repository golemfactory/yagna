//! Requestor control part of Activity API
use crate::web::WebClient;
use crate::Result;
use std::sync::Arc;
use ya_model::activity::{ExeScriptCommandResult, ExeScriptRequest};

/// Bindings for Requestor Control part of the Activity API.
pub struct RequestorControlApiClient {
    client: WebClient,
}

impl RequestorControlApiClient {
    pub fn new(client: WebClient) -> Self {
        Self { client }
    }

    /// Creates new Activity based on given Agreement.
    pub async fn create_activity(&self, agreement_id: &str) -> Result<String> {
        self.client
            .post("activity")
            .send_json(&agreement_id)
            .json()
            .await
    }

    /// Destroys given Activity.
    pub async fn destroy_activity(&self, activity_id: &str) -> Result<()> {
        let uri = url_format!("activity/{activity_id}", activity_id);
        self.client.delete(&uri).send().json().await?;
        Ok(())
    }

    /// Executes an ExeScript batch within a given Activity.
    pub async fn exec(&self, script: ExeScriptRequest, activity_id: &str) -> Result<String> {
        let uri = url_format!("activity/{activity_id}/exec", activity_id);
        self.client.post(&uri).send_json(&script).json().await
    }

    /// Queries for ExeScript batch results.
    #[rustfmt::skip]
    pub async fn get_exec_batch_results(
        &self,
        activity_id: &str,
        batch_id: &str,
        timeout: Option<i32>,
        #[allow(non_snake_case)]
        maxCount: Option<i32>,
    ) -> Result<Vec<ExeScriptCommandResult>> {
        let uri = url_format!(
            "activity/{activity_id}/exec/{batch_id}",
            activity_id,
            batch_id,
            #[query] timeout,
            #[query] maxCount
        );
        self.client.get(&uri).send().json().await
    }
}
