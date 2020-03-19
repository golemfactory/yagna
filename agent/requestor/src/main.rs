use actix_rt::Arbiter;
use futures::{channel::mpsc, prelude::*};
use std::ops::Not;
use std::time::Duration;
use structopt::StructOpt;
use url::Url;

use ya_client::{
    activity::ActivityRequestorControlApi, market::MarketRequestorApi,
    payment::requestor::RequestorApi as PaymentApi, web::WebClient,
};
//use ya_model::market::proposal::State;
use chrono::Utc;
use std::collections::HashSet;
use ya_model::activity::ExeScriptRequest;
use ya_model::market::{proposal::State, AgreementProposal, Demand, Proposal, RequestorEvent};
use ya_model::payment::{Acceptance, Allocation, EventType};

#[derive(StructOpt)]
struct AppSettings {
    /// Authorization token to server
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    app_key: String,

    ///
    #[structopt(
        long = "market-url",
        env = "YAGNA_MARKET_URL",
        default_value = "http://10.30.10.202:5001/market-api/v1/"
    )]
    market_url: Url,

    ///
    #[structopt(long = "activity-url", env = "YAGNA_ACTIVITY_URL")]
    activity_url: Option<Url>,

    #[structopt(long = "payment-url", env = "YAGNA_PAYMENT_URL")]
    payment_url: Option<Url>,
}

impl AppSettings {
    fn market_api(&self) -> Result<ya_client::market::MarketRequestorApi, anyhow::Error> {
        Ok(WebClient::with_token(&self.app_key)?.interface_at(self.market_url.clone()))
    }

    fn activity_api(&self) -> Result<ActivityRequestorControlApi, anyhow::Error> {
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
) -> Result<ProcessOfferResult, anyhow::Error> {
    let proposal_id = offer.proposal_id()?.clone();

    if offer.state.unwrap_or(State::Initial) == State::Initial {
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
    tx: futures::channel::mpsc::Sender<String>,
) -> Result<(), anyhow::Error> {
    loop {
        let events = requestor_api
            .collect(&subscription_id, Some(2.0), Some(5))
            .await?;

        if !events.is_empty() {
            log::debug!("market events={:#?}", events);
        } else {
            tokio::time::delay_for(Duration::from_millis(3000)).await;
        }
        for event in events {
            match event {
                RequestorEvent::ProposalEvent {
                    event_date: _,
                    proposal,
                } => {
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
    activity_api: &ActivityRequestorControlApi,
    agreement_id: String,
) -> Result<(), anyhow::Error> {
    log::info!("GOT new agreement = {}", agreement_id);

    let act_id = activity_api.create_activity(&agreement_id).await?;
    log::info!("GOT new activity = (({})); YAY!", act_id);

    tokio::time::delay_for(Duration::from_secs(30)).await;

    log::info!("destroying activity = (({})); AGRRR!", act_id);
    activity_api.destroy_activity(&act_id).await?;
    log::info!("I'M DONE FOR NOW");

    activity_api
        .exec(ExeScriptRequest::new("".to_string()), &act_id)
        .await
        .unwrap();
    Ok(())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();
    let started_at = Utc::now();
    let settings = AppSettings::from_args();

    let payment_api = settings.payment_api()?;

    let node_name = "test1";

    let my_demand = build_demand(node_name);
    //(golem.runtime.wasm.wasi.version@v=*)

    let allocation = Allocation {
        allocation_id: "".to_string(),
        total_amount: 10.into(),
        spent_amount: Default::default(),
        remaining_amount: Default::default(),
        timeout: None,
        make_deposit: false,
    };
    let new_allocation = payment_api.create_allocation(&allocation).await.unwrap();

    let market_api = settings.market_api()?;
    let subscription_id = market_api.subscribe(&my_demand).await?;

    log::info!("sub_id={}", subscription_id);

    let mkt_api = market_api.clone();
    let sub_id = subscription_id.clone();
    Arbiter::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        mkt_api.unsubscribe(&sub_id).await.unwrap();
    });

    let mkt_api = market_api.clone();
    let sub_id = subscription_id.clone();
    let (tx, mut rx) = mpsc::channel::<String>(1);
    Arbiter::spawn(async move {
        if let Err(e) = spawn_workers(mkt_api, &sub_id, &my_demand, tx).await {
            log::error!("spawning workers for {} error: {}", sub_id, e);
        }
    });

    // log incoming debit notes
    {
        let payment_api = payment_api.clone();
        let mut ts = started_at.clone();
        Arbiter::spawn(async move {
            loop {
                let next_ts = Utc::now();
                let events = match payment_api.get_debit_note_events(Some(&ts)).await {
                    Err(e) => {
                        log::error!("fail get debit notes events: {}", e);
                        break;
                    }
                    Ok(events) => events,
                };

                for event in events {
                    log::info!("got debit note event {:?}", event);
                }
                ts = next_ts;
            }
        })
    }

    let activity_api = settings.activity_api()?;
    let mut agreements_to_pay = HashSet::new();
    if let Some(id) = rx.next().await {
        if let Err(e) = process_agreement(&activity_api, id.clone()).await {
            log::error!("processing agreement id {} error: {}", id, e);
        }
        let terminate_result = market_api.terminate_agreement(&id).await;
        log::info!("agreement: {}, terminated: {:?}", id, terminate_result);
        agreements_to_pay.insert(id);
    }

    let mut ts = started_at;

    while agreements_to_pay.is_empty().not() {
        let next_ts = Utc::now();

        let events = payment_api.get_invoice_events(Some(&ts)).await.unwrap();
        // TODO: timeout on get_invoice_events does not work
        if events.is_empty() {
            tokio::time::delay_for(Duration::from_millis(5000)).await;
        }

        for event in events {
            match event.event_type {
                EventType::Received => {
                    let invoice = payment_api.get_invoice(&event.invoice_id).await.unwrap();
                    let acceptance = Acceptance {
                        total_amount_accepted: invoice.amount,
                        allocation_id: new_allocation.allocation_id.clone(),
                    };
                    let result = payment_api
                        .accept_invoice(&event.invoice_id, &acceptance)
                        .await;
                    log::info!("payment acceptance result: {:?}", result);
                }
                _ => (),
            }
            ts = next_ts;
        }
    }

    market_api.unsubscribe(&subscription_id).await?;
    Ok(())
}
