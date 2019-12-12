use actix_rt::Runtime;
use futures::prelude::*;
use ya_client::{
    activity::{
        provider::ProviderApiClient, RequestorControlApiClient, RequestorStateApiClient, API_ROOT,
    },
    web::WebClient,
    Result,
};
use ya_model::activity::{activity_state::State, ActivityState, ActivityUsage, ExeScriptRequest};

fn new_client() -> Result<WebClient> {
    WebClient::builder().api_root(API_ROOT).build()
}

async fn provider(activity_id: &str) -> Result<()> {
    let client = ProviderApiClient::new(new_client()?);

    println!("[?] Events for activity {}", activity_id);
    let activity_events = client.get_activity_events(Some(60i32)).await.unwrap();
    println!("[<] Events: {:?}", activity_events);

    let activity_state = ActivityState::new(State::Ready);
    println!("[+] Setting activity state to: {:?}", activity_state);
    client
        .set_activity_state(activity_id, activity_state)
        .await
        .unwrap();
    println!("[<] Done");

    let activity_usage = ActivityUsage::new(Some(vec![10f64, 0.5f64]));
    println!("[+] Setting activity usage to: {:?}", activity_usage);
    client
        .set_activity_usage(activity_id, activity_usage)
        .await
        .unwrap();
    println!("[<] Done");
    Ok(())
}

async fn requestor(agreement_id: &str) -> Result<()> {
    let activity_id = requestor_start(agreement_id).await?;
    requestor_exec(&activity_id).await?;
    requestor_state(&activity_id).await?;
    requestor_stop(&activity_id).await
}

async fn requestor_start(agreement_id: &str) -> Result<String> {
    let client = RequestorControlApiClient::new(new_client()?);

    println!("[+] Activity, agreement {}", agreement_id);
    let activity_id = client.create_activity(agreement_id).await.unwrap();
    println!("[<] Activity: {}", activity_id);

    Ok(activity_id)
}

async fn requestor_stop(activity_id: &str) -> Result<()> {
    let client = RequestorControlApiClient::new(new_client()?);

    println!("[-] Activity {}", activity_id);
    client.destroy_activity(&activity_id).await.unwrap();
    println!("[<] Destroyed");
    Ok(())
}

async fn requestor_exec(activity_id: &str) -> Result<()> {
    let client = RequestorControlApiClient::new(new_client()?);

    let exe_request = ExeScriptRequest::new("STOP".to_string());
    println!("[+] Batch exe script:{:?}", exe_request);
    let batch_id = client.exec(&activity_id, exe_request).await.unwrap();
    println!("[<] Batch id: {}", batch_id);

    println!("[?] Batch results for activity {}", activity_id);
    let results = client
        .get_exec_batch_results(&activity_id, &batch_id, Some(3), Some(10i32))
        .await
        .unwrap();
    println!("[<] Batch results: {:?}", results);
    Ok(())
}

async fn requestor_state(activity_id: &str) -> Result<()> {
    let client = RequestorStateApiClient::new(new_client()?);

    println!("[?] State for activity {}", activity_id);
    let state = client.get_state(activity_id).await.unwrap();
    println!("[<] State: {:?}", state);

    println!("[?] Usage vector for activity {}", activity_id);
    let usage = client.get_usage(activity_id).await.unwrap();
    println!("[<] Usage vector: {:?}", usage);

    println!("[?] Command state for activity {}", activity_id);
    let command_state = client.get_running_command(activity_id).await.unwrap();
    println!("[<] Command state: {:?}", command_state);
    Ok(())
}

async fn interact() -> Result<()> {
    requestor("agreement_id").await?;
    provider("activity_id").await
}

fn main() {
    Runtime::new()
        .expect("Cannot create runtime")
        .block_on(interact().boxed_local().compat())
        .expect("Runtime error");
}
