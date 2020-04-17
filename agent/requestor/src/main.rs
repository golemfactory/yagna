use actix_rt::{signal, Arbiter};
use chrono::Utc;
use futures::{channel::mpsc, prelude::*};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::{convert::TryInto, path::PathBuf};
use structopt::{clap, StructOpt};

use ya_client::{cli::ApiOpts, cli::RequestorApi, Error};

mod activity;
mod market;
mod payment;

const DEFAULT_NODE_NAME: &str = "test1";
const DEFAULT_TASK_PACKAGE: &str = "hash://sha3:38D951E2BD2408D95D8D5E5068A69C60C8238FA45DB8BC841DC0BD50:http://34.244.4.185:8000/rust-wasi-tutorial.zip";

#[derive(StructOpt)]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
struct AppSettings {
    #[structopt(flatten)]
    api: ApiOpts,
    #[structopt(long = "exe-script")]
    exe_script: PathBuf,
    #[structopt(long = "node-name", default_value = DEFAULT_NODE_NAME)]
    node_name: String,
    #[structopt(long = "task-package", default_value = DEFAULT_TASK_PACKAGE)]
    task_package: String,
    #[structopt(long = "allocation-size", default_value = "100")]
    allocation_size: i64,
}

/// if needed unsubscribes from the market and releases allocation
async fn shutdown_handler(
    activities: Arc<Mutex<HashSet<String>>>,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
    subscription_id: String,
    api: RequestorApi,
) {
    signal::ctrl_c().await.unwrap();

    log::info!("terminating...");

    let activities = std::mem::replace(&mut (*activities.lock().unwrap()), HashSet::new());
    let agreement_allocation =
        std::mem::replace(&mut (*agreement_allocation.lock().unwrap()), HashMap::new());

    log::info!("unsubscribing demand...");
    let mut pending = vec![api
        .market
        .unsubscribe(&subscription_id)
        .map(|_| Ok(()))
        .map_err(move |e: Error| log::error!("unable to unsubscribe the demand: {:?}", e))
        .boxed_local()];

    if activities.len() > 0 {
        log::info!("destroying activities ({}) ...", activities.len());
        pending.extend(activities.iter().map(|id| {
            log::debug!("destroying activity {}", id);
            api.activity
                .control()
                .destroy_activity(&id)
                .map_err(move |e| log::error!("unable to destroy activity {}: {:?}", id, e))
                .boxed_local()
        }));
    }

    if agreement_allocation.len() > 0 {
        log::info!("releasing allocations ({}) ...", agreement_allocation.len());
        pending.extend(
            agreement_allocation
                .iter()
                .map(|(agreement_id, allocation_id)| {
                    // TODO: we need to terminate the agreement first (Market service does not support it yet)
                    // api.market.terminate_agreement(&agreement_id).await;
                    log::debug!(
                        "releasing allocation {} for {}",
                        allocation_id,
                        agreement_id
                    );
                    api.payment
                        .release_allocation(&allocation_id)
                        .map_err(move |e| {
                            log::error!("unable to release allocation {}: {:?}", allocation_id, e)
                        })
                        .boxed_local()
                }),
        );
    }

    futures::future::join_all(pending.into_iter()).await;

    //TODO: maybe even accept invoice
    match (activities.len(), agreement_allocation.len()) {
        (0, 0) => log::info!("cleanly terminated."),
        (act, 0) => log::warn!("terminated.\n\n {} activity(ies) destroyed prematurely.", act),
        (0, alloc) => log::warn!("terminated.\n\n {} agreement(s) possibly not settled.", alloc),
        (act, alloc) => log::warn!(
            "terminated.\n\n {} activity(ies) destroyed prematurely and {} agreement(s) possibly not settled.",
            act,
            alloc
        ),
    }

    Arbiter::current().stop();
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let started_at = Utc::now();
    let settings = AppSettings::from_args();
    let api: RequestorApi = settings.api.try_into()?;

    let exe_script = std::fs::read_to_string(&settings.exe_script)?;
    let commands_cnt = match serde_json::from_str(&exe_script)? {
        serde_json::Value::Array(arr) => arr.len(),
        _ => return Err(anyhow::anyhow!("Command list is empty")),
    };

    let activities = Arc::new(Mutex::new(HashSet::new()));
    let agreement_allocation = Arc::new(Mutex::new(HashMap::new()));
    let my_demand = market::build_demand(&settings.node_name, &settings.task_package);

    let subscription_id = api.market.subscribe(&my_demand).await?;
    log::info!("\n\n DEMAND SUBSCRIBED: {}", subscription_id);

    let shutdown = shutdown_handler(
        activities.clone(),
        agreement_allocation.clone(),
        subscription_id.clone(),
        api.clone(),
    );

    let (agreement_tx, mut agreement_rx) = mpsc::channel::<String>(1);
    {
        let api = api.clone();
        let subscription_id = subscription_id.clone();
        let allocation_size = settings.allocation_size;
        let agreement_allocation = agreement_allocation.clone();
        Arbiter::spawn(async move {
            if let Err(e) = market::spawn_negotiations(
                &api,
                &subscription_id,
                &my_demand,
                allocation_size,
                agreement_allocation,
                agreement_tx,
            )
            .await
            {
                log::error!("spawning negotiation for {} error: {}", subscription_id, e);
            }
        });
    }

    let payment_api = api.payment.clone();
    Arbiter::spawn(payment::log_and_ignore_debit_notes(
        payment_api.clone(),
        started_at.clone(),
    ));

    Arbiter::spawn(payment::process_payments(
        payment_api.clone(),
        started_at,
        agreement_allocation.clone(),
    ));

    Arbiter::spawn(async move {
        while let Some(agreement_id) = agreement_rx.next().await {
            Arbiter::spawn(activity::spawn_activity(
                api.clone(),
                agreement_id,
                exe_script.clone(),
                commands_cnt,
                activities.clone(),
            ))
        }
    });

    shutdown.await;
    Ok(())
}
