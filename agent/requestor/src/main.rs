use actix_rt::{signal, Arbiter};
use chrono::{DateTime, Utc};
use futures::{channel::mpsc, prelude::*};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::{convert::TryInto, path::PathBuf, time::Duration};
use structopt::{clap, StructOpt};

use ya_client::{
    activity::ActivityRequestorApi, cli::ApiOpts, market::MarketRequestorApi,
    payment::requestor::RequestorApi as PaymentRequestorApi, RequestorApi,
};
use ya_model::{
    activity::ExeScriptRequest,
    market::{
        proposal::State as ProposalState, AgreementProposal, Demand, Proposal, RequestorEvent,
    },
    payment::{Acceptance, EventType, NewAllocation, Rejection, RejectionReason},
};

#[derive(StructOpt)]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
struct AppSettings {
    #[structopt(flatten)]
    api: ApiOpts,
    #[structopt(long = "exe-script")]
    exe_script: PathBuf,
    #[structopt(long = "allocation-amount", default_value = "100")]
    allocation_amount: i64,
}

enum ProcessOfferResult {
    ProposalId(String),
    AgreementId(String),
}

async fn process_offer(
    requestor_api: MarketRequestorApi,
    offer: Proposal,
    subscription_id: &str,
    my_demand: Demand,
) -> anyhow::Result<ProcessOfferResult> {
    let proposal_id = offer.proposal_id()?.clone();

    if offer.state.unwrap_or(ProposalState::Initial) == ProposalState::Initial {
        if offer.prev_proposal_id.is_some() {
            anyhow::bail!("Proposal in Initial state but with prev id: {:#?}", offer)
        }
        let bespoke_proposal = offer.counter_demand(my_demand)?;
        let new_proposal_id = requestor_api
            .counter_proposal(&bespoke_proposal, subscription_id)
            .await?;
        return Ok(ProcessOfferResult::ProposalId(new_proposal_id));
    }

    let new_agreement_id = proposal_id;
    let new_agreement = AgreementProposal::new(
        new_agreement_id.clone(),
        Utc::now() + chrono::Duration::hours(2),
    );
    let _ack = requestor_api.create_agreement(&new_agreement).await?;
    log::info!("\n\n confirming via AGREEMENT: {}", new_agreement_id);
    requestor_api.confirm_agreement(&new_agreement_id).await?;
    log::info!("\n\n waiting for agreement approval: {}", new_agreement_id);
    requestor_api
        .wait_for_approval(&new_agreement_id, Some(7.879))
        .await?;
    log::info!("\n\n AGREEMENT APPROVED: {} !", new_agreement_id);

    Ok(ProcessOfferResult::AgreementId(new_agreement_id))
}

async fn spawn_workers(
    market_api: MarketRequestorApi,
    subscription_id: &str,
    my_demand: &Demand,
    agreement_tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    loop {
        let events = market_api
            .collect(&subscription_id, Some(5.0), Some(5))
            .await?;

        if !events.is_empty() {
            log::debug!("got {} market events", events.len());
        }
        for event in events {
            match event {
                RequestorEvent::ProposalEvent {
                    event_date: _,
                    proposal,
                } => {
                    log::debug!(
                        "\n\n got ProposalEvent [{}]; state: {:?}",
                        proposal.proposal_id()?,
                        proposal.state
                    );
                    log::trace!("proposal: {:#?}", proposal);
                    let mut agreement_tx = agreement_tx.clone();
                    let requestor_api = market_api.clone();
                    let my_subs_id = subscription_id.to_string();
                    let my_demand = my_demand.clone();
                    Arbiter::spawn(async move {
                        match process_offer(requestor_api, proposal, &my_subs_id, my_demand).await {
                            Ok(ProcessOfferResult::ProposalId(id)) => {
                                log::info!("\n\n ACCEPTED via counter proposal [{}]", id)
                            }
                            Ok(ProcessOfferResult::AgreementId(id)) => {
                                agreement_tx.send(id).await.unwrap()
                            }
                            Err(e) => {
                                log::error!("unable to process offer: {}", e);
                                return;
                            }
                        }
                    });
                }
                _ => {
                    log::warn!("invalid response");
                }
            }
        }
    }
}

async fn spawn_activity(
    agreement_id: String,
    exe_script: String,
    commands_cnt: usize,
    alloc_amount: i64,
    activities: Arc<Mutex<HashSet<String>>>,
    allocations: Arc<Mutex<HashMap<String, String>>>,
    api: RequestorApi,
) {
    let new_allocation = NewAllocation {
        total_amount: alloc_amount.into(),
        timeout: None,
        make_deposit: false,
    };

    match api.payment.create_allocation(&new_allocation).await {
        Ok(alloc) => {
            log::info!(
                "allocated {} GNT ({})",
                alloc.total_amount,
                alloc.allocation_id
            );
            allocations
                .lock()
                .unwrap()
                .insert(agreement_id.clone(), alloc.allocation_id);
        }
        Err(err) => {
            log::error!("unable to allocate GNT: {:?}", err);
            match api.market.cancel_agreement(&agreement_id).await {
                Ok(_) => log::warn!("agreement {} cancelled", agreement_id),
                Err(e) => log::error!("unable to cancel agreement {}: {}", agreement_id, e),
            }
            return;
        }
    };

    let fut = run_activity(
        &api.activity,
        agreement_id.clone(),
        exe_script,
        commands_cnt,
        activities,
    );

    if let Err(e) = fut.await {
        log::error!("error processing agreement {}: {}", agreement_id, e);
    }
    // TODO: Market doesn't support agreement termination yet.
    // let terminate_result = market_api.terminate_agreement(&id).await;
    // log::info!("agreement: {}, terminated: {:?}", id, terminate_result);
}

fn build_demand(node_name: &str) -> Demand {
    Demand {
        properties: serde_json::json!({
            "golem": {
                "node": {
                    "id": {
                        "name": node_name
                    },
                    "ala": 1
                },
                "srv": {
                    "comp":{
                        "wasm": {
                            "task_package": "http://34.244.4.185:8000/rust-wasi-tutorial.zip"
                        }
                    }
                }
            }
        }),
        constraints: r#"(&
            (golem.inf.mem.gib>0.5)
            (golem.inf.storage.gib>1)
            (golem.com.pricing.model=linear)
        )"#
        .to_string(),

        demand_id: Default::default(),
        requestor_id: Default::default(),
    }
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
    activities.lock().unwrap().remove(&act_id);
    activity_api.control().destroy_activity(&act_id).await?;
    log::info!("\n\n ACTIVITY DESTROYED.");

    Ok(())
}

/// MOCK: log incoming debit notes, and... ignore them
async fn log_and_ignore_debit_notes(payment_api: PaymentRequestorApi, started_at: DateTime<Utc>) {
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at.clone();

    loop {
        match payment_api.get_debit_note_events(Some(&events_after)).await {
            Err(e) => {
                log::error!("getting debit notes events error: {}", e);
                tokio::time::delay_for(Duration::from_secs(5)).await;
            }
            Ok(events) => {
                for event in events {
                    log::info!("got debit note event {:?}", event);
                    events_after = event.timestamp;
                }
            }
        }
    }
}

async fn process_payments(
    payment_api: PaymentRequestorApi,
    started_at: DateTime<Utc>,
    allocations: Arc<Mutex<HashMap<String, String>>>,
) {
    log::info!("\n\n waiting for INVOICES");
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at;

    loop {
        let events = match payment_api.get_invoice_events(Some(&events_after)).await {
            Err(e) => {
                log::error!("getting invoice events error: {}", e);
                tokio::time::delay_for(Duration::from_secs(5)).await;
                vec![]
            }
            Ok(events) => events,
        };

        for event in events {
            log::info!("got INVOICE event {:#?}", event);
            match event.event_type {
                EventType::Received => Arbiter::spawn(process_invoice(
                    event.invoice_id,
                    allocations.clone(),
                    payment_api.clone(),
                )),
                _ => log::warn!(
                    "ignoring event type {:?} for: {}",
                    event.event_type,
                    event.invoice_id
                ),
            }
            events_after = event.timestamp;
        }
    }
}

async fn process_invoice(
    invoice_id: String,
    allocations: Arc<Mutex<HashMap<String, String>>>,
    payment_api: PaymentRequestorApi,
) {
    let mut invoice = payment_api.get_invoice(&invoice_id).await;
    while let Err(e) = invoice {
        log::error!("retry getting invoice {} after error: {}", invoice_id, e);
        tokio::time::delay_for(Duration::from_secs(5)).await;
        invoice = payment_api.get_invoice(&invoice_id).await;
    }

    let invoice = invoice.unwrap();
    log::debug!("got INVOICE: {:#?}", invoice);

    let allocation = allocations
        .lock()
        .unwrap()
        .get(&invoice.agreement_id)
        .cloned();

    match allocation {
        Some(allocation_id) => {
            let acceptance = Acceptance {
                total_amount_accepted: invoice.amount,
                allocation_id: allocation_id.clone(),
            };
            match payment_api.accept_invoice(&invoice_id, &acceptance).await {
                // TODO: reconsider what to do in this case: probably retry
                Err(e) => log::error!("accepting invoice {}, error: {}", invoice_id, e),
                Ok(_) => log::info!("\n\n INVOICE ACCEPTED: {:?}", invoice_id),
            }

            allocations.lock().unwrap().remove(&invoice.agreement_id);
            match payment_api.release_allocation(&allocation_id).await {
                Ok(_) => log::info!("released allocation {}", allocation_id),
                Err(e) => log::error!("Unable to release allocation {}: {}", allocation_id, e),
            }
        }
        None => {
            let rejection = Rejection {
                rejection_reason: RejectionReason::UnsolicitedService,
                total_amount_accepted: 0.into(),
                message: None,
            };
            match payment_api.reject_invoice(&invoice_id, &rejection).await {
                Err(e) => log::error!("rejecting invoice {}, error: {}", invoice_id, e),
                Ok(_) => log::warn!("invoice rejected: {:?}", invoice_id),
            }
        }
    }
}

/// if needed unsubscribes from the market and releases allocation
async fn shutdown_handler(
    activities: Arc<Mutex<HashSet<String>>>,
    allocations: Arc<Mutex<HashMap<String, String>>>,
    subscription_id: String,
    market_api: MarketRequestorApi,
    activity_api: ActivityRequestorApi,
    payment_api: PaymentRequestorApi,
) {
    signal::ctrl_c().await.unwrap();

    log::info!("terminating...");

    let activities = std::mem::replace(&mut (*activities.lock().unwrap()), HashSet::new());
    let allocations = std::mem::replace(&mut (*allocations.lock().unwrap()), HashMap::new());

    log::info!("unsubscribing demand...");
    let mut pending = vec![market_api
        .unsubscribe(&subscription_id)
        .map(|_| Ok(()))
        .map_err(|e| log::error!("unable to unsubscribe the demand: {:?}", e))
        .boxed_local()];

    log::info!("destroying activities ({}) ...", activities.len());
    pending.extend(activities.iter().map(|id| {
        activity_api
            .control()
            .destroy_activity(&id)
            .map_err(|e| log::error!("unable to destroy activity {}: {:?}", id, e))
            .boxed_local()
    }));

    log::info!("releasing allocations ({}) ...", allocations.len());
    pending.extend(allocations.iter().map(|(_, id)| {
        payment_api
            .release_allocation(&id)
            .map_err(|e| log::error!("unable to release allocation {}: {:?}", id, e))
            .boxed_local()
    }));

    futures::future::join_all(pending.into_iter()).await;

    //TODO: maybe even accept invoice
    log::debug!("shutdown...");
    Arbiter::current().stop()
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let started_at = Utc::now();
    let settings = AppSettings::from_args();
    let api: RequestorApi = settings.api.try_into()?;

    let amount = settings.allocation_amount;
    let exe_script = std::fs::read_to_string(&settings.exe_script)?;
    let commands_cnt = match serde_json::from_str(&exe_script)? {
        serde_json::Value::Array(arr) => arr.len(),
        _ => return Err(anyhow::anyhow!("Command list is empty")),
    };

    let activities = Arc::new(Mutex::new(HashSet::new()));
    let allocations = Arc::new(Mutex::new(HashMap::new()));

    let my_demand = build_demand("test1");
    let subscription_id = api.market.subscribe(&my_demand).await?;
    log::info!("\n\n DEMAND SUBSCRIBED: {}", subscription_id);

    let shutdown = shutdown_handler(
        activities.clone(),
        allocations.clone(),
        subscription_id.clone(),
        api.market.clone(),
        api.activity.clone(),
        api.payment.clone(),
    );

    let (agreement_tx, mut agreement_rx) = mpsc::channel::<String>(1);
    let market_api = api.market.clone();
    let payment_api = api.payment.clone();
    let sub_id = subscription_id.clone();

    Arbiter::spawn(async move {
        if let Err(e) = spawn_workers(market_api, &sub_id, &my_demand, agreement_tx).await {
            log::error!("spawning workers for {} error: {}", sub_id, e);
        }
    });

    Arbiter::spawn(log_and_ignore_debit_notes(
        payment_api.clone(),
        started_at.clone(),
    ));

    Arbiter::spawn(process_payments(
        payment_api.clone(),
        started_at,
        allocations.clone(),
    ));

    Arbiter::spawn(async move {
        while let Some(agreement_id) = agreement_rx.next().await {
            Arbiter::spawn(spawn_activity(
                agreement_id,
                exe_script.clone(),
                commands_cnt,
                amount,
                activities.clone(),
                allocations.clone(),
                api.clone(),
            ))
        }
    });

    shutdown.await;
    Ok(())
}
