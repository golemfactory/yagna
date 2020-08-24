use actix_rt::{signal, Arbiter};
use chrono::Utc;
use futures::{channel::mpsc, prelude::*};
use humantime::Duration;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use structopt::{clap, StructOpt};

use std::convert::TryFrom;
use ya_client::{cli::ApiOpts, cli::RequestorApi, Error};

mod activity;
mod market;
mod payment;

const DEFAULT_NODE_NAME: &str = "test1";
const DEFAULT_TASK_PACKAGE: &str = "hash://sha3:D5E31B2EED628572A5898BF8C34447644BFC4B5130CFC1E4F10AEAA1:http://3.249.139.167:8000/rust-wasi-tutorial.zip";

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::crate_version_commit!())]
struct AppSettings {
    #[structopt(flatten)]
    api: ApiOpts,
    #[structopt(long)]
    exe_script: PathBuf,
    /// Subnetwork identifier. You can set this value to filter nodes
    /// with other identifiers than selected. Useful for test purposes.
    #[structopt(long, env = "SUBNET")]
    pub subnet: Option<String>,
    #[structopt(long, default_value = DEFAULT_NODE_NAME)]
    node_name: String,
    #[structopt(long, default_value = DEFAULT_TASK_PACKAGE)]
    task_package: String,
    #[structopt(long, default_value = "100")]
    allocation_size: i64,
    /// Estimated time limit for requested task completion. All agreements will expire
    /// after specified time counted from demand subscription. All activities will
    /// be destroyed, when agreement expires.
    ///
    /// It is not well specified, what to do with payment after agreement expiration.
    /// There are many scenarios, eg.:
    /// - Requestor requested bigger work than feasible to compute within this limit
    ///
    /// - Provider was not as performant as he declared
    #[structopt(long, default_value = "15min")]
    pub task_expiration: Duration,
    /// Exit after processing one agreement.
    #[structopt(long)]
    one_agreement: bool,
}

/// if needed unsubscribes from the market and releases allocation
async fn shutdown(
    activities: Arc<Mutex<HashSet<String>>>,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
    subscription_id: String,
    api: RequestorApi,
) {
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

fn shutdown_handler(
    activities: Arc<Mutex<HashSet<String>>>,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
    subscription_id: String,
    api: RequestorApi,
) -> (impl Future, futures::future::AbortHandle) {
    let (future_ctrl_c, ctrl_c_abort) = futures::future::abortable(signal::ctrl_c());

    let shutdown_fut = async {
        match future_ctrl_c.await {
            Ok(Ok(())) => log::info!("Caught ctrl-c"),
            Ok(Err(error)) => log::error!("Failed listening to ctrl-c: {}", error),
            Err(futures::future::Aborted) => {
                log::info!("Finished processing one Agreement. Payment is confirmed. Finishing")
            }
        }
        shutdown(activities, agreement_allocation, subscription_id, api).await;
    };

    return (shutdown_fut, ctrl_c_abort);
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or("info".into()),
    );
    env_logger::init();

    let started_at = Utc::now();
    let settings = AppSettings::from_args();
    let api = RequestorApi::try_from(&settings.api)?;

    let exe_script = std::fs::read_to_string(&settings.exe_script)?;
    let commands_cnt = match serde_json::from_str(&exe_script)? {
        serde_json::Value::Array(arr) => arr.len(),
        _ => return Err(anyhow::anyhow!("Command list is empty")),
    };

    let activities = Arc::new(Mutex::new(HashSet::new()));
    let agreement_allocation = Arc::new(Mutex::new(HashMap::new()));
    let my_demand = market::build_demand(
        &settings.node_name,
        &settings.task_package,
        chrono::Duration::from_std(*settings.task_expiration)?,
        &settings.subnet,
    );

    log::debug!(
        "Demand created: {}",
        serde_json::to_string_pretty(&my_demand).unwrap()
    );

    let subscription_id = api.market.subscribe(&my_demand).await?;
    log::info!("\n\n DEMAND SUBSCRIBED: {}", subscription_id);

    let (shutdown_fut, app_abort_handle) = shutdown_handler(
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
        match settings.one_agreement {
            true => Some(app_abort_handle.clone()),
            false => None,
        },
    ));

    Arbiter::spawn(async move {
        while let Some(agreement_id) = agreement_rx.next().await {
            Arbiter::spawn(activity::spawn_activity(
                api.clone(),
                agreement_id,
                exe_script.clone(),
                commands_cnt,
                activities.clone(),
            ));
            if settings.one_agreement {
                break;
            }
        }
    });

    shutdown_fut.await;
    Ok(())
}
