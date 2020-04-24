use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use ya_client::model::activity::ExeScriptRequest;
use ya_client::{activity::ActivityRequestorApi, cli::RequestorApi};

pub(crate) async fn spawn_activity(
    api: RequestorApi,
    agreement_id: String,
    exe_script: String,
    commands_cnt: usize,
    activities: Arc<Mutex<HashSet<String>>>,
) {
    let fut = run_activity(
        &api.activity,
        agreement_id.clone(),
        exe_script,
        commands_cnt,
        activities,
    );

    if let Err(e) = fut.await {
        log::error!(
            "processing activity for agreement {} failed: {}",
            agreement_id,
            e
        );
    }
    // TODO: Market doesn't support agreement termination yet.
    // let terminate_result = market_api.terminate_agreement(&id).await;
    // log::info!("agreement: {}, terminated: {:?}", id, terminate_result);
}

async fn run_activity(
    activity_api: &ActivityRequestorApi,
    agreement_id: String,
    exe_script: String,
    commands_cnt: usize,
    activities: Arc<Mutex<HashSet<String>>>,
) -> anyhow::Result<()> {
    log::info!("creating activity for agreement = {}", agreement_id);

    let act_id = activity_api
        .control()
        .create_activity(&agreement_id)
        .await?;

    activities.lock().unwrap().insert(act_id.clone());
    log::info!("\n\n ACTIVITY CREATED: {}; YAY!", act_id);
    log::info!("\n\n executing script with {} commands", commands_cnt);

    let batch_id = activity_api
        .control()
        .exec(ExeScriptRequest::new(exe_script), &act_id)
        .await?;
    log::info!("\n\n EXE SCRIPT called, batch_id: {}", batch_id);

    let mut results = Vec::new();

    loop {
        let state = activity_api.state().get_state(&act_id).await?;
        if !state.alive() {
            log::info!("activity {} is NOT ALIVE any more.", act_id);
            break;
        }

        log::info!(
            "activity state: {:?}. Waiting for batch to complete...",
            state
        );
        results = activity_api
            .control()
            .get_exec_batch_results(&act_id, &batch_id, Some(7.), None)
            .await?;

        if results.len() >= commands_cnt {
            log::info!("\n\n BATCH COMPLETED: {:#?}", results);
            break;
        }
    }

    if results.len() < commands_cnt {
        log::warn!("\n\n BATCH INTERRUPTED: {:#?}", results);
    }

    log::info!("\n\n destroying activity: {}; ", act_id);
    activity_api.control().destroy_activity(&act_id).await?;
    activities.lock().unwrap().remove(&act_id);
    log::info!("\n\n ACTIVITY DESTROYED.");

    Ok(())
}
