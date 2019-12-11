use crate::activity::web::{QueryParamsBuilder, WebClient};
use crate::error::Error;
use crate::Result;
use futures::compat::Future01CompatExt;
use futures::prelude::*;
use std::mem;
use ya_model::activity::{ExeScriptCommandResult, ExeScriptRequest};

pub struct RequestorControlApiClient {
    client: WebClient,
}

impl RequestorControlApiClient {
    pub fn new(client: WebClient) -> Self {
        Self { client }
    }

    pub fn replace_client(&mut self, client: WebClient) {
        mem::replace(&mut self.client, client);
    }
}

impl RequestorControlApiClient {
    pub async fn create_activity(&self, agreement_id: &str) -> Result<String> {
        let url = format!("{}/activity", self.client.endpoint);
        let mut response = self
            .client
            .awc
            .post(&url)
            .send_json(&agreement_id)
            .compat()
            .map_err(Error::from)
            .await?;

        match response.json().compat().await {
            Ok(result) => Ok(result),
            Err(e) => Err(Error::from(e)),
        }
    }

    pub async fn destroy_activity(&self, activity_id: &str) -> Result<()> {
        let url = format!("{}/activity/{}", self.client.endpoint, activity_id);
        self.client
            .awc
            .delete(&url)
            .send()
            .compat()
            .map_err(Error::from)
            .await?;

        Ok(())
    }

    pub async fn exec(&self, activity_id: &str, script: ExeScriptRequest) -> Result<String> {
        let url = format!("{}/activity/{}/exec", self.client.endpoint, activity_id);
        let mut response = self
            .client
            .awc
            .post(&url)
            .send_json(&script)
            .compat()
            .map_err(Error::from)
            .await?;

        match response.json().compat().await {
            Ok(result) => Ok(result),
            Err(e) => Err(Error::from(e)),
        }
    }

    pub async fn get_exec_batch_results(
        &self,
        activity_id: &str,
        batch_id: &str,
        timeout: Option<i32>,
        max_count: Option<i32>,
    ) -> Result<Vec<ExeScriptCommandResult>> {
        let params = QueryParamsBuilder::new()
            .put("timeout", timeout)
            .put("max_count", max_count)
            .build();
        let url = format!(
            "{}/activity/{}/exec/{}?{}",
            self.client.endpoint, activity_id, batch_id, params
        );
        let mut response = self
            .client
            .awc
            .get(&url)
            .send()
            .compat()
            .map_err(Error::from)
            .await?;

        match response.json().compat().await {
            Ok(result) => Ok(result),
            Err(e) => Err(Error::from(e)),
        }
    }
}
