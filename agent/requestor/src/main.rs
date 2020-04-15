use actix_rt::{signal, Arbiter};
use chrono::{DateTime, Utc};
use futures::{channel::mpsc, channel::oneshot, future, prelude::*};
use std::{convert::TryInto, path::PathBuf, time::Duration};
use structopt::{clap, StructOpt};

use ya_client::{
    activity::ActivityRequestorApi, cli::ApiOpts, market::MarketRequestorApi,
    payment::requestor::RequestorApi as PaymentRequestorApi, cli::RequestorApi,
};
use ya_model::{
    activity::ExeScriptRequest,
    market::{
        proposal::State as ProposalState, AgreementProposal, Demand, Proposal, RequestorEvent,
    },
    payment::{Acceptance, Allocation, EventType, NewAllocation, Rejection, RejectionReason},
};

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

fn build_demand(node_name: &str, task_package: &str) -> Demand {
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
                            "task_package": task_package
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
    exe_script: &PathBuf,
) -> anyhow::Result<()> {
    log::info!("creating activity for agreement = {}", agreement_id);

    let act_id = activity_api
        .control()
        .create_activity(&agreement_id)
        .await?;
    log::info!("\n\n ACTIVITY CREATED: {}; YAY!", act_id);

    let contents = std::fs::read_to_string(&exe_script)?;
    let commands_cnt = match serde_json::from_str(&contents)? {
        serde_json::Value::Array(arr) => {
            log::info!("\n\n Executing script with {} commands", arr.len());
            arr.len()
        }
        _ => 0,
    };

    let batch_id = activity_api
        .control()
        .exec(ExeScriptRequest::new(contents), &act_id)
        .await?;
    log::info!("\n\n EXE SCRIPT called, batch_id: {}", batch_id);

    loop {
        let state = activity_api.state().get_state(&act_id).await?;
        if !state.alive() {
            log::info!("activity {} is NOT ALIVE any more.", act_id);
            break;
        }

        log::info!(
            "Activity state: {:?}. Waiting for batch to complete...",
            state
        );
        let results = activity_api
            .control()
            .get_exec_batch_results(&act_id, &batch_id, Some(7.), None)
            .await?;

        log::info!("\n\n BATCH COMPLETED. Results: {:#?}", results);

        if results.len() >= commands_cnt {
            break;
        }
    }

    log::info!("\n\n AGRRR! destroying activity: {}; ", act_id);
    activity_api.control().destroy_activity(&act_id).await?;
    log::info!("\n\n ACTIVITY DESTROYED.");

    Ok(())
}

/// MOCK: fixed price allocation
async fn allocate_funds_for_task(
    payment_api: &PaymentRequestorApi,
    allocation_tx: oneshot::Sender<String>,
) -> anyhow::Result<Allocation> {
    let new_allocation = NewAllocation {
        total_amount: 100.into(),
        timeout: None,
        make_deposit: false,
    };
    let allocation = payment_api.create_allocation(&new_allocation).await?;
    log::info!("Allocated {} GNT.", &allocation.total_amount);

    let allocation_id = allocation.allocation_id.clone();
    allocation_tx.send(allocation_id).unwrap();

    Ok(allocation)
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

/// MOCK: accept all incoming invoices
async fn process_payments(
    payment_api: PaymentRequestorApi,
    allocation: Allocation,
    started_at: DateTime<Utc>,
    finished_agreement_id: String,
) {
    log::info!(
        "\n\n waiting for INVOICE for finished agreement = {}",
        finished_agreement_id
    );
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at;

    'infinite_loop: loop {
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
            let invoice_id = &event.invoice_id;
            match event.event_type {
                EventType::Received => {
                    let mut invoice = payment_api.get_invoice(invoice_id).await;
                    while let Err(e) = invoice {
                        log::error!("retry getting invoice {} after error: {}", invoice_id, e);
                        tokio::time::delay_for(Duration::from_secs(5)).await;
                        invoice = payment_api.get_invoice(invoice_id).await;
                    }

                    let invoice = invoice.unwrap();
                    log::debug!("got INVOICE: {:#?}", invoice);
                    if invoice.agreement_id != finished_agreement_id {
                        let rejection = Rejection {
                            rejection_reason: RejectionReason::UnsolicitedService,
                            total_amount_accepted: 0.into(),
                            message: None,
                        };
                        match payment_api.reject_invoice(invoice_id, &rejection).await {
                            Err(e) => log::error!("rejecting invoice {}, error: {}", invoice_id, e),
                            Ok(_) => log::warn!("invoice rejected: {:?}", invoice_id),
                        }
                        continue;
                    }

                    let acceptance = Acceptance {
                        total_amount_accepted: invoice.amount,
                        allocation_id: allocation.allocation_id.clone(),
                    };
                    match payment_api.accept_invoice(invoice_id, &acceptance).await {
                        // TODO: reconsider what to do in this case: probably retry
                        Err(e) => log::error!("accepting invoice {}, error: {}", invoice_id, e),
                        Ok(_) => log::info!("\n\n INVOICE ACCEPTED: {:?}", invoice_id),
                    }
                    break 'infinite_loop; // we just want to accept one invoice for the finished agreement
                }
                _ => log::warn!(
                    "ignoring event type {:?} for: {}",
                    event.event_type,
                    invoice_id
                ),
            }
            events_after = event.timestamp;
        }
    }
}

/// if needed unsubscribes from the market and releases allocation
async fn shutdown_handler(
    mut allocation_rx: oneshot::Receiver<String>,
    mut subscription_rx: oneshot::Receiver<String>,
    invoice_rx: oneshot::Receiver<()>,
    shutdown_tx: oneshot::Sender<()>,
    market_api: MarketRequestorApi,
    payment_api: PaymentRequestorApi,
) {
    future::select(signal::ctrl_c().boxed_local(), invoice_rx).await;
    log::info!("terminating...");
    if let Ok(Some(allocation_id)) = allocation_rx.try_recv() {
        let a = &allocation_id;
        log::info!("releasing allocation...");
        payment_api.release_allocation(&a).await.unwrap();
    }
    if let Ok(Some(subscription_id)) = subscription_rx.try_recv() {
        log::info!("unsubscribing demand...");
        market_api.unsubscribe(&subscription_id).await.unwrap();
    }
    //TODO: destroy started activity
    //TODO: maybe even accept invoice

    log::debug!("shutdown...");
    shutdown_tx.send(()).unwrap();
    Arbiter::current().stop()
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();
    let started_at = Utc::now();
    let settings = AppSettings::from_args();
    let api: RequestorApi = settings.api.try_into()?;

    let (allocation_tx, allocation_rx) = oneshot::channel();
    let (subscription_tx, subscription_rx) = oneshot::channel();
    let (invoice_tx, invoice_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let payment_api = api.payment.clone();
    let market_api = api.market.clone();
    Arbiter::spawn(shutdown_handler(
        allocation_rx,
        subscription_rx,
        invoice_rx,
        shutdown_tx,
        market_api,
        payment_api,
    ));

    let allocation = allocate_funds_for_task(&api.payment, allocation_tx).await?;
    let my_demand = build_demand(&settings.node_name, &settings.task_package);
    //(golem.runtime.wasm.wasi.version@v=*)

    let subscription_id = api.market.subscribe(&my_demand).await?;
    subscription_tx.send(subscription_id.clone()).unwrap();

    log::info!("\n\n DEMAND SUBSCRIBED: {}", subscription_id);

    let mkt_api = api.market.clone();
    let sub_id = subscription_id.clone();
    let (agreement_tx, mut agreement_rx) = mpsc::channel::<String>(1);
    Arbiter::spawn(async move {
        if let Err(e) = spawn_workers(mkt_api, &sub_id, &my_demand, agreement_tx).await {
            log::error!("spawning workers for {} error: {}", sub_id, e);
        }
    });

    Arbiter::spawn(log_and_ignore_debit_notes(
        api.payment.clone(),
        started_at.clone(),
    ));

    let exe_script = settings.exe_script.clone();

    Arbiter::spawn(async move {
        let mut finished_agreement_id = "".to_string();
        while let Some(agreement_id) = agreement_rx.next().await {
            match run_activity(&api.activity, agreement_id.clone(), &exe_script).await {
                Ok(_) => {
                    finished_agreement_id = agreement_id;
                    break;
                }
                Err(e) => log::error!("processing agreement id {} error: {}", agreement_id, e),
            }
            // TODO: Market doesn't support agreement termination yet.
            // let terminate_result = market_api.terminate_agreement(&id).await;
            // log::info!("agreement: {}, terminated: {:?}", id, terminate_result);
        }

        process_payments(
            api.payment.clone(),
            allocation,
            started_at,
            finished_agreement_id,
        )
        .await;

        invoice_tx.send(()).unwrap();
        log::info!("\n\n I'M DONE FOR NOW");
    });

    shutdown_rx.await.unwrap();
    log::debug!("THE END.");
    Ok(())
}
