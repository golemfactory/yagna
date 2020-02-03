#![allow(unused_imports)]

use actix_rt::{Arbiter, System};
use chrono::{TimeZone, Utc};
use futures::channel::mpsc;
use futures::prelude::*;
use structopt::StructOpt;
use url::Url;
use ya_client::{market::MarketRequestorApi, web::WebClient};

use ya_model::market::event::RequestorEvent;
use ya_model::market::{AgreementProposal, Demand, Proposal};

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
    activity_url: Url,
}

impl AppSettings {
    fn market_api(
        &self,
    ) -> Result<ya_client::market::MarketRequestorApi, Box<dyn std::error::Error>> {
        Ok(WebClient::with_token(&self.app_key)?.interface_at(self.market_url.clone()))
    }

    fn activity_api(
        &self,
    ) -> Result<ya_client::activity::ActivityRequestorControlApi, Box<dyn std::error::Error>> {
        Ok(WebClient::with_token(&self.app_key)?.interface_at(self.activity_url.clone()))
    }
}

async fn process_offer(
    requestor_api: MarketRequestorApi,
    offer: Proposal,
) -> Result<String, Box<dyn std::error::Error>> {
    let agreement_id = offer.proposal_id.unwrap().clone();
    let agreement = AgreementProposal::new(
        agreement_id.clone(),
        Utc.ymd(2021, 1, 15).and_hms(9, 10, 11),
    );
    let _ack = requestor_api.create_agreement(&agreement).await?;
    log::info!("confirm agreement = {}", agreement_id);
    requestor_api.confirm_agreement(&agreement_id).await?;
    log::info!("wait for agreement = {}", agreement_id);
    requestor_api.wait_for_approval(&agreement_id).await?;

    Ok(agreement_id)
}

async fn spawn_workers(
    requestor_api: MarketRequestorApi,
    subscription_id: &str,
    tx: futures::channel::mpsc::Sender<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let events = requestor_api
            .collect(&subscription_id, Some(120), Some(5))
            .await?;

        if !events.is_empty() {
            log::debug!("events={:?}", events);
        }
        for event in events {
            match event {
                RequestorEvent::ProposalEvent {
                    event_date: _,
                    proposal,
                } => {
                    let mut tx = tx.clone();
                    let requestor_api = requestor_api.clone();
                    Arbiter::spawn(async move {
                        let agreement_id = match process_offer(requestor_api, proposal).await {
                            Ok(id) => id,
                            Err(e) => {
                                log::error!("unable to process offer: {}", e);
                                return;
                            }
                        };
                        tx.send(agreement_id.clone()).await.unwrap();
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

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::try_init()?;

    let settings = AppSettings::from_args();

    let node_name = "test1";

    let demand = build_demand(node_name);
    //(golem.runtime.wasm.wasi.version@v=*)

    let market_api = settings.market_api()?;
    let subscription_id = market_api.subscribe(&demand).await?;

    eprintln!("sub_id={}", subscription_id);

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
            log::info!("new agreement = {}", id);
            let act_id = activity_api.create_activity(&id).await.unwrap();
            log::info!("new activity = (({}))", act_id);
            //activity_api.exec(ExeScriptRequest::new("".to_string()), &act_id).await.unwrap();
        }
    });
    spawn_workers(requestor_api.clone(), &subscription_id, tx).await?;

    market_api.unsubscribe(&subscription_id).await?;
    Ok(())
}
