use actix_rt::Arbiter;
use chrono::{DateTime, Utc};
use futures::{channel::mpsc, prelude::*};
use std::{path::PathBuf, time::Duration};
use structopt::StructOpt;
use url::Url;

use ya_client::payment::requestor::RequestorApi as PaymentApi;
use ya_client::{
    activity::ActivityRequestorApi, market::MarketRequestorApi, web::WebClient, web::WebInterface,
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
    market_url: Url,

    /// Activity API URL
    #[structopt(long = "activity-url", env = ActivityRequestorApi::API_URL_ENV_VAR)]
    activity_url: Option<Url>,

    #[structopt(long = "payment-url", env = "YAGNA_PAYMENT_URL")]
    payment_url: Option<Url>,

    #[structopt(long = "exe-script")]
    exe_script: PathBuf,
}

impl AppSettings {
    fn market_api(&self) -> Result<ya_client::market::MarketRequestorApi, anyhow::Error> {
        Ok(WebClient::with_token(&self.app_key)?.interface_at(self.market_url.clone()))
    }

    fn activity_api(&self) -> Result<ActivityRequestorApi, anyhow::Error> {
        let client = WebClient::with_token(&self.app_key)?;
        if let Some(url) = &self.activity_url {
            Ok(client.interface_at(url.clone()))
        } else {
            Ok(client.interface()?)
        }
    }

    fn payment_api(&self) -> Result<PaymentApi, anyhow::Error> {
        let client = WebClient::with_token(&self.app_key)?;
        if let Some(url) = &self.payment_url {
            Ok(client.interface_at(url.clone()))
        } else {
            Ok(client.interface()?)
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
    log::info!("confirm agreement = {}", new_agreement_id);
    requestor_api.confirm_agreement(&new_agreement_id).await?;
    log::info!("wait for agreement = {}", new_agreement_id);
    requestor_api
        .wait_for_approval(&new_agreement_id, Some(7.879))
        .await?;
    log::info!("agreement = {} CONFIRMED!", new_agreement_id);

    Ok(ProcessOfferResult::AgreementId(new_agreement_id))
}

async fn spawn_workers(
    requestor_api: MarketRequestorApi,
    subscription_id: &str,
    my_demand: &Demand,
    tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    loop {
        let events = requestor_api
            .collect(&subscription_id, Some(2.0), Some(5))
            .await?;

        if !events.is_empty() {
            log::debug!("got {} market events", events.len());
        } else {
            tokio::time::delay_for(Duration::from_millis(3000)).await;
        }
        for event in events {
            match event {
                RequestorEvent::ProposalEvent {
                    event_date: _,
                    proposal,
                } => {
                    log::debug!(
                        "processing ProposalEvent [{:?}] with state: {:?}",
                        proposal.proposal_id,
                        proposal.state
                    );
                    log::trace!("processing proposal {:?}", proposal);
                    let mut tx = tx.clone();
                    let requestor_api = requestor_api.clone();
                    let my_subs_id = subscription_id.to_string();
                    let my_demand = my_demand.clone();
                    Arbiter::spawn(async move {
                        match process_offer(requestor_api, proposal, &my_subs_id, my_demand).await {
                            Ok(ProcessOfferResult::ProposalId(id)) => {
                                log::info!("responded with counter proposal (id: {})", id)
                            }
                            Ok(ProcessOfferResult::AgreementId(id)) => tx.send(id).await.unwrap(),
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
                            "task_package": "http://localhost:8000/rust-wasi-tutorial.zip"
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

async fn process_agreement(
    activity_api: &ActivityRequestorApi,
    agreement_id: String,
    exe_script: &PathBuf,
    mut tx: mpsc::Sender<()>,
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
            log::info!("\n\n Executing script {} commands", arr.len());
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

        log::info!("activity {} state: {:?}", act_id, state);
        let results = activity_api
            .control()
            .get_exec_batch_results(&act_id, &batch_id, Some(7))
            .await?;

        log::info!("batch results {:?}", results);

        if results.len() >= commands_cnt {
            break;
        }

        tokio::time::delay_for(Duration::from_millis(700)).await;
    }

    //    tokio::time::delay_for(Duration::from_millis(7000)).await;

    log::info!("\n\n AGRRR! destroying activity: {}; ", act_id);
    activity_api.control().destroy_activity(&act_id).await?;
    log::info!("\n\n I'M DONE FOR NOW");

    tx.send(()).await?;
    Ok(())
}

/// MOCK: fixed price allocation
async fn allocate_funds_for_task(payment_api: &PaymentApi) -> anyhow::Result<Allocation> {
    let new_allocation = NewAllocation {
        total_amount: 10.into(),
        timeout: None,
        make_deposit: false,
    };
    let allocation = payment_api.create_allocation(&new_allocation).await?;
    log::info!("Allocated {} GNT.", &allocation.total_amount);
    Ok(allocation)
}

/// MOCK: log incoming debit notes, and... ignore them
async fn log_and_ignore_debit_notes(
    payment_api: &PaymentApi,
    started_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let mut ts = started_at.clone();
    loop {
        let next_ts = Utc::now();
        let events = payment_api.get_debit_note_events(Some(&ts)).await?;

        for event in events {
            log::info!("got debit note event {:#?}", event);
        }
        ts = next_ts;
        tokio::time::delay_for(Duration::from_secs(5)).await;
    }
}

/// MOCK: accept all incoming invoices
async fn process_payments(
    payment_api: &PaymentApi,
    allocation: Allocation,
    started_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let mut ts = started_at;
    loop {
        let next_ts = Utc::now();

        let events = payment_api.get_invoice_events(Some(&ts)).await?;
        // TODO: timeout on get_invoice_events does not work
        if events.is_empty() {
            tokio::time::delay_for(Duration::from_secs(10)).await;
        }

        for event in events {
            log::info!("got invoice event {:#?}", event);
            match event.event_type {
                EventType::Received => {
                    let invoice = payment_api.get_invoice(&event.invoice_id).await?;
                    let acceptance = Acceptance {
                        total_amount_accepted: invoice.amount,
                        allocation_id: allocation.allocation_id.clone(),
                    };
                    payment_api
                        .accept_invoice(&event.invoice_id, &acceptance)
                        .await?;
                    log::info!("invoice accepted: {:?}", event.invoice_id);
                }
                _ => log::info!(
                    "ignoring event type {:?} for: {}",
                    event.event_type,
                    event.invoice_id
                ),
            }
            ts = next_ts;
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

    let node_name = "test1";
    let my_demand = build_demand(node_name);
    //(golem.runtime.wasm.wasi.version@v=*)

    let market_api = settings.market_api()?;
    let subscription_id = market_api.subscribe(&my_demand).await?;

    log::info!("sub_id={}", subscription_id);

    // mount signal handler to unsubscribe from the market
    {
        let market_api = market_api.clone();
        let sub_id = subscription_id.clone();
        Arbiter::spawn(async move {
            tokio::signal::ctrl_c().await.unwrap();
            market_api.unsubscribe(&sub_id).await.unwrap();
            // TODO: destroy running activity
            // TODO: process (accept / reject) incoming payments
        });
    }

    let mkt_api = market_api.clone();
    let sub_id = subscription_id.clone();
    let (tx, mut rx) = mpsc::channel::<String>(1);
    Arbiter::spawn(async move {
        if let Err(e) = spawn_workers(mkt_api, &sub_id, &my_demand, tx).await {
            log::error!("spawning workers for {} error: {}", sub_id, e);
        }
    });

    let activity_api = settings.activity_api()?;
    let exe_script = settings.exe_script.clone();
    let (agreement_tx, mut _agreement_rx) = mpsc::channel::<()>(1);
    Arbiter::spawn(async move {
        while let Some(id) = rx.next().await {
            let agreement_tx = agreement_tx.clone();
            if let Err(e) =
                process_agreement(&activity_api, id.clone(), &exe_script, agreement_tx).await
            {
                log::error!("processing agreement id {} error: {}", id, e);
            }
            // TODO: Market doesn't support agreement termination yet.
            // let terminate_result = market_api.terminate_agreement(&id).await;
            // log::info!("agreement: {}, terminated: {:?}", id, terminate_result);
        }
    });

    // log incoming debit notes
    {
        let payment_api = payment_api.clone();
        let started_at = started_at.clone();
        Arbiter::spawn(async move {
            if let Err(e) = log_and_ignore_debit_notes(&payment_api, started_at).await {
                log::error!("logging debit notes error: {}", e);
            }
        })
    }

    Arbiter::spawn(async move {
        if let Err(e) = process_payments(&payment_api, allocation, started_at).await {
            log::error!("processing payments error: {}", e)
        }
    });

    // waiting only for first agreement to be fully processed
    // agreement_rx.next().await;
    tokio::signal::ctrl_c().await?;
    settings.market_api()?.unsubscribe(&subscription_id).await?;
    Ok(())
}
