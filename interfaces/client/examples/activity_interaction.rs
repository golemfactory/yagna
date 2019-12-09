use actix_rt::Runtime;
use async_trait::async_trait;
use awc::Client;
use failure::{Fail, Fallible};
use futures::compat::Future01CompatExt;
use futures::prelude::*;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use ya_client::activity::provider::ProviderApi;
use ya_model::activity::activity_state::State;
use ya_model::activity::{ActivityState, ActivityUsage, ProblemDetails, ProviderEvent};

const API_ENDPOINT: &str = "http://localhost:5001/activity-api/v1";

#[derive(Debug, Deserialize, Fail, Serialize)]
pub enum ActivityApiError {
    #[fail(display = "Transport error: {:?}", details)]
    ClientError { details: ProblemDetails },
}

impl From<awc::error::SendRequestError> for ActivityApiError {
    fn from(err: awc::error::SendRequestError) -> Self {
        let details = ProblemDetails::new("request".to_string(), format!("{:?}", err));
        ActivityApiError::ClientError { details }
    }
}

impl From<awc::error::JsonPayloadError> for ActivityApiError {
    fn from(err: awc::error::JsonPayloadError) -> Self {
        let details = ProblemDetails::new("response".to_string(), format!("{:?}", err));
        ActivityApiError::ClientError { details }
    }
}

struct HttpProviderApiClient;

#[async_trait(?Send)]
impl ProviderApi for HttpProviderApiClient {
    async fn get_activity_events(&self, timeout: Option<i32>) -> Fallible<Vec<ProviderEvent>> {
        let mut url = format!("{}/activity/events", API_ENDPOINT);
        if let Some(timeout) = timeout {
            url = format!("{}?timeout={}", url, timeout);
        }

        let mut response = Client::default()
            .get(&url)
            .send()
            .compat()
            .map_err(ActivityApiError::from)
            .await?;

        match response.json().compat().await {
            Ok(events) => Ok(events),
            Err(e) => Err(ActivityApiError::from(e).into()),
        }
    }

    async fn set_activity_state(&self, activity_id: &str, state: ActivityState) -> Fallible<()> {
        let url = format!("{}/activity/{}/state", API_ENDPOINT, activity_id);
        Client::default()
            .put(&url)
            .send_json(&state)
            .compat()
            .map_err(ActivityApiError::from)
            .await?;
        Ok(())
    }

    async fn set_activity_usage(&self, activity_id: &str, usage: ActivityUsage) -> Fallible<()> {
        let url = format!("{}/activity/{}/usage", API_ENDPOINT, activity_id);
        Client::default()
            .put(&url)
            .send_json(&usage)
            .compat()
            .map_err(ActivityApiError::from)
            .await?;
        Ok(())
    }
}

async fn interact() -> () {
    let client = HttpProviderApiClient {};
    let activity_id = "activity";

    let activity_events = client.get_activity_events(Some(60i32)).await.unwrap();
    println!("Activity events: {:?}", activity_events);

    let activity_state = ActivityState::new(State::Ready);
    println!("Setting activity state to: {:?}", activity_state);
    client
        .set_activity_state(activity_id, activity_state)
        .await
        .unwrap();

    let activity_usage = ActivityUsage::new(Some(vec![10f64, 0.5f64]));
    println!("Setting activity usage to: {:?}", activity_usage);
    client
        .set_activity_usage(activity_id, activity_usage)
        .await
        .unwrap();
}

fn main() {
    Runtime::new()
        .expect("Cannot create runtime")
        .block_on(interact().boxed_local().unit_error().compat())
        .expect("Runtime error");
}
