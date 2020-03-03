use actix_rt::Arbiter;
use futures::{channel::mpsc, prelude::*};
use std::time::Duration;
use structopt::StructOpt;
use url::Url;

use ya_client::{
    activity::ActivityRequestorControlApi, market::MarketRequestorApi, web::WebClient,
};
//use ya_model::market::proposal::State;
use ya_model::market::{proposal::State, AgreementProposal, Demand, Proposal, RequestorEvent};

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
        let bespoke_proposal = Proposal::from_demand(&offer, &my_demand);
        let new_proposal_id = requestor_api
            .counter_proposal(&bespoke_proposal, subscription_id)
            .await?;
        return Ok(ProcessOfferResult::ProposalId(new_proposal_id));
    }

    let new_agreement_id = proposal_id;
    let new_agreement = AgreementProposal::new(
        new_agreement_id.clone(),
        "2021-01-01T18:54:16.655397Z".parse()?,
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
            .collect(&subscription_id, Some(12.0), Some(5))
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

    tokio::time::delay_for(Duration::from_millis(7000)).await;

    log::info!("destroying activity = (({})); AGRRR!", act_id);
    activity_api.destroy_activity(&act_id).await?;
    log::info!("I'M DONE FOR NOW");

    //activity_api.exec(ExeScriptRequest::new("".to_string()), &act_id).await.unwrap();
    Ok(())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let settings = AppSettings::from_args();

    let node_name = "test1";

    let my_demand = build_demand(node_name);
    //(golem.runtime.wasm.wasi.version@v=*)

    let market_api = settings.market_api()?;
    let subscription_id = market_api.subscribe(&my_demand).await?;

    log::info!("sub_id={}", subscription_id);

    {
        let requestor_api = market_api.clone();
        let subscription_id = subscription_id.clone();
        Arbiter::spawn(async move {
            tokio::signal::ctrl_c().await.unwrap();
            requestor_api.unsubscribe(&subscription_id).await.unwrap();
        })
    }
    let requestor_api = market_api.clone();
    let activity_api = settings.activity_api()?;

    let (tx, mut rx): (mpsc::Sender<String>, mpsc::Receiver<String>) =
        futures::channel::mpsc::channel(1);
    Arbiter::spawn(async move {
        while let Some(id) = rx.next().await {
            if let Err(e) = process_agreement(&activity_api, id.clone()).await {
                log::error!("processing agreement id {} error: {}", id, e);
                return;
            }
        }
    });
    spawn_workers(requestor_api.clone(), &subscription_id, &my_demand, tx).await?;

    market_api.unsubscribe(&subscription_id).await?;
    Ok(())
}
