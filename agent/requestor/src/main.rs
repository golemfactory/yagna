use actix_rt::{signal, Arbiter};
use chrono::{DateTime, Utc};
use futures::{channel::mpsc, channel::oneshot, future, prelude::*};
use std::{path::PathBuf, time::Duration};
use structopt::StructOpt;
use url::Url;

use ya_client::payment::requestor::RequestorApi as PaymentRequestorApi;
use ya_client::{
    activity::ActivityRequestorApi,
    market::MarketRequestorApi,
    web::{WebClient, WebInterface},
};
use ya_model::{
    activity::ExeScriptRequest,
    market::{
        proposal::State as ProposalState, AgreementProposal, Demand, Proposal, RequestorEvent,
    },
    payment::{Acceptance, Allocation, EventType, NewAllocation},
};

#[derive(StructOpt)]
struct AppSettings {
    /// Authorization token to server
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    app_key: String,

    /// Market API URL
    #[structopt(long = "market-url", env = MarketRequestorApi::API_URL_ENV_VAR)]
    market_url: Option<Url>,

    /// Activity API URL
    #[structopt(long = "activity-url", env = ActivityRequestorApi::API_URL_ENV_VAR)]
    activity_url: Option<Url>,

    #[structopt(long = "payment-url", env = PaymentRequestorApi::API_URL_ENV_VAR)]
    payment_url: Option<Url>,

    #[structopt(long = "exe-script")]
    exe_script: PathBuf,

    /// Subnetwork identifier. You can set this value to filter nodes
    /// with other identifiers than selected. Useful for test purposes.
    #[structopt(long = "subnet", env = "SUBNET")]
    pub subnet: Option<String>,
}

impl AppSettings {
    fn market_api(&self) -> anyhow::Result<MarketRequestorApi> {
        self.api_client(&self.market_url)
    }

    fn activity_api(&self) -> anyhow::Result<ActivityRequestorApi> {
        self.api_client(&self.activity_url)
    }

    fn payment_api(&self) -> anyhow::Result<PaymentRequestorApi> {
        self.api_client(&self.payment_url)
    }

    pub fn api_client<T: WebInterface>(&self, url: &Option<Url>) -> anyhow::Result<T> {
        let client = WebClient::with_token(&self.app_key)?;
        match url.as_ref() {
            Some(url) => Ok(client.interface_at(url.clone())),
            None => Ok(client.interface()?),
        }
    }
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
    log::info!("\n\nconfirming agreement = {}", new_agreement_id);
    requestor_api.confirm_agreement(&new_agreement_id).await?;
    log::info!("\n\nwaiting for agreement = {}", new_agreement_id);
    requestor_api
        .wait_for_approval(&new_agreement_id, Some(7.879))
        .await?;
    log::info!("\n\nagreement = {} CONFIRMED!", new_agreement_id);

    Ok(ProcessOfferResult::AgreementId(new_agreement_id))
}

async fn spawn_workers(
    requestor_api: MarketRequestorApi,
    subscription_id: &str,
    my_demand: &Demand,
    agreement_tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    loop {
        let events = requestor_api
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
                    let requestor_api = requestor_api.clone();
                    let my_subs_id = subscription_id.to_string();
                    let my_demand = my_demand.clone();
                    Arbiter::spawn(async move {
                        match process_offer(requestor_api, proposal, &my_subs_id, my_demand).await {
                            Ok(ProcessOfferResult::ProposalId(id)) => {
                                log::info!("\n\n responded with counter proposal [{}]", id)
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

fn build_demand(node_name: &str, task_package: &str, subnet: &Option<String>) -> Demand {
    // Task should expire in 10 minutes.
    let expiration = Utc::now() + chrono::Duration::minutes(10);
    let mut properties = serde_json::json!({
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
                    },
                    "expiration": expiration.timestamp_millis()
                }
            }
        }
    });

    let subnet_constraint = match subnet {
        Some(subnet) => {
            properties.as_object_mut().unwrap().insert(
                "golem.node.debug.subnet".to_string(),
                serde_json::Value::String(subnet.clone()),
            );
            format!("(golem.node.debug.subnet={})", subnet.clone())
        }
        None => "".to_string(),
    };

    Demand {
        properties,
        constraints: format!(
            "(&
            (golem.inf.mem.gib>0.5)
            (golem.inf.storage.gib>1)
            (golem.com.pricing.model=linear)
            {}
        )",
            subnet_constraint
        ),

        demand_id: Default::default(),
        requestor_id: Default::default(),
    }
}

async fn process_agreement(
    activity_api: &ActivityRequestorApi,
    agreement_id: String,
    exe_script: &PathBuf,
) -> anyhow::Result<()> {
    log::info!("\n\n processing AGREEMENT = {}", agreement_id);

    let act_id = activity_api
        .control()
        .create_activity(&agreement_id)
        .await?;
    log::info!("\n\n created new ACTIVITY: {}; YAY!", act_id);

    let contents = std::fs::read_to_string(&exe_script)?;
    let commands_cnt = match serde_json::from_str(&contents)? {
        serde_json::Value::Array(arr) => {
            log::info!("\n\n Executing script: {} commands", arr.len());
            arr.len()
        }
        _ => 0,
    };

    let batch_id = activity_api
        .control()
        .exec(ExeScriptRequest::new(contents), &act_id)
        .await?;
    log::info!("got BATCH_ID: {}", batch_id);

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

        log::info!("Batch results {:#?}", results);

        if results.len() >= commands_cnt {
            break;
        }
    }

    log::info!("\n\n AGRRR! destroying activity: {}; ", act_id);
    activity_api.control().destroy_activity(&act_id).await?;
    log::info!("\n\n I'M DONE FOR NOW");

    Ok(())
}

/// MOCK: fixed price allocation
async fn allocate_funds_for_task(payment_api: &PaymentRequestorApi) -> anyhow::Result<Allocation> {
    let new_allocation = NewAllocation {
        total_amount: 100.into(),
        timeout: None,
        make_deposit: false,
    };
    let allocation = payment_api.create_allocation(&new_allocation).await?;
    log::info!("Allocated {} GNT.", &allocation.total_amount);
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
                    log::info!("got debit note event {:#?}", event);
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
) {
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
            log::info!("got invoice event {:#?}", event);
            match event.event_type {
                EventType::Received => {
                    let invoice = payment_api.get_invoice(&event.invoice_id).await;
                    if let Err(e) = invoice {
                        log::error!("getting invoice {}, err: {}", event.invoice_id, e);
                        // TODO: loop until you've got proper invoice
                        continue;
                    }
                    let invoice = invoice.unwrap();

                    let acceptance = Acceptance {
                        total_amount_accepted: invoice.amount,
                        allocation_id: allocation.allocation_id.clone(),
                    };
                    match payment_api
                        .accept_invoice(&event.invoice_id, &acceptance)
                        .await
                    {
                        Err(e) => {
                            log::error!("accepting invoice {}, err: {}", event.invoice_id, e);
                            // TODO: reconsider what to do in this case
                            continue;
                        }
                        Ok(_) => log::info!("invoice accepted: {:?}", event.invoice_id),
                    }
                }
                _ => log::info!(
                    "ignoring event type {:?} for: {}",
                    event.event_type,
                    event.invoice_id
                ),
            }
            events_after = event.timestamp;
        }
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();
    let started_at = Utc::now();
    let settings = AppSettings::from_args();

    let payment_api = settings.payment_api()?;
    let allocation = allocate_funds_for_task(&payment_api).await?;
    let allocation_id = allocation.allocation_id.clone();

    let node_name = "test1";
    let task_package = "hash://sha3:38D951E2BD2408D95D8D5E5068A69C60C8238FA45DB8BC841DC0BD50:http://34.244.4.185:8000/rust-wasi-tutorial.zip";
    let my_demand = build_demand(node_name, task_package, &settings.subnet);
    //(golem.runtime.wasm.wasi.version@v=*)

    let market_api = settings.market_api()?;
    let subscription_id = market_api.subscribe(&my_demand).await?;

    log::info!("sub_id={}", subscription_id);

    let mkt_api = market_api.clone();
    let sub_id = subscription_id.clone();
    let (agreement_tx, mut agreement_rx) = mpsc::channel::<String>(1);
    Arbiter::spawn(async move {
        if let Err(e) = spawn_workers(mkt_api, &sub_id, &my_demand, agreement_tx).await {
            log::error!("spawning workers for {} error: {}", sub_id, e);
        }
    });

    let activity_api = settings.activity_api()?;
    let exe_script = settings.exe_script.clone();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    Arbiter::spawn(async move {
        while let Some(id) = agreement_rx.next().await {
            match process_agreement(&activity_api, id.clone(), &exe_script).await {
                Ok(_) => {
                    shutdown_tx.send(()).unwrap();
                    break;
                }
                Err(e) => log::error!("processing agreement id {} error: {}", id, e),
            }
            // TODO: Market doesn't support agreement termination yet.
            // let terminate_result = market_api.terminate_agreement(&id).await;
            // log::info!("agreement: {}, terminated: {:?}", id, terminate_result);
        }
    });

    Arbiter::spawn(log_and_ignore_debit_notes(
        payment_api.clone(),
        started_at.clone(),
    ));

    Arbiter::spawn(process_payments(
        payment_api.clone(),
        allocation,
        started_at,
    ));

    future::select(signal::ctrl_c().boxed_local(), shutdown_rx).await;

    market_api.unsubscribe(&subscription_id).await?;
    payment_api.release_allocation(&allocation_id).await?;
    // TODO: process (accept / reject) incoming payments

    Arbiter::current().stop();
    Ok(())
}
