use futures::{compat::Future01CompatExt, TryFutureExt};
use std::mem;

use crate::{web::WebClient, Error, Result};
use ya_model::activity::{ActivityState, ExeScriptCommandState};

pub struct RequestorStateApiClient {
    client: WebClient,
}

impl RequestorStateApiClient {
    pub fn new(client: WebClient) -> Self {
        Self { client }
    }

    pub fn replace_client(&mut self, client: WebClient) {
        mem::replace(&mut self.client, client);
    }
}

impl RequestorStateApiClient {
    pub async fn get_running_command(&self, activity_id: &str) -> Result<ExeScriptCommandState> {
        let endpoint = self.client.configuration.api_endpoint("/activity");
        let url = format!("{}/{}/command", endpoint, activity_id);
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
            Err(e) => Err(Error::from(e).into()),
        }
    }

    pub async fn get_state(&self, activity_id: &str) -> Result<ActivityState> {
        let endpoint = self.client.configuration.api_endpoint("/activity");
        let url = format!("{}/{}/state", endpoint, activity_id);
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
            Err(e) => Err(Error::from(e).into()),
        }
    }

    pub async fn get_usage(&self, activity_id: &str) -> Result<Vec<f64>> {
        let endpoint = self.client.configuration.api_endpoint("/activity");
        let url = format!("{}/{}/usage", endpoint, activity_id);
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
            Err(e) => Err(Error::from(e).into()),
        }
    }
}
