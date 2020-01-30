use actix_rt::Arbiter;
use futures::channel::mpsc;
use futures::prelude::*;
use structopt::StructOpt;
use url::Url;
use ya_client::web::WebAuth;
use ya_client::{
    market::{ApiClient, ProviderApi, RequestorApi},
    web::WebClient,
};
use ya_model::activity::ExeScriptRequest;
use ya_model::market::{Agreement, Demand, Offer, Proposal, ProviderEvent, RequestorEvent};

#[derive(StructOpt)]
struct AppSettings {
    /// Authorization token to server
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    app_key: String,

    ///
    #[structopt(long = "market-url", env = "YAGNA_MARKET_URL")]
    market_url: Url,

    ///
    #[structopt(long = "activity-url", env = "YAGNA_ACTIVITY_URL")]
    activity_url: Url,
}

impl AppSettings {
    fn market_api(&self) -> Result<ya_client::market::ApiClient, Box<dyn std::error::Error>> {
        let host_port = format!(
            "{}:{}",
            self.market_url.host_str().unwrap_or_default(),
            self.market_url.port_or_known_default().unwrap_or_default()
        );

        let connection = WebClient::builder()
            .auth(WebAuth::Bearer(self.app_key.clone()))
            .host_port(host_port);

        Ok(ApiClient::new(connection)?)
    }

    fn activity_api(
        &self,
    ) -> Result<ya_client::activity::RequestorControlApiClient, Box<dyn std::error::Error>> {
        let host_port = format!(
            "{}:{}",
            self.activity_url.host_str().unwrap_or_default(),
            self.activity_url
                .port_or_known_default()
                .unwrap_or_default()
        );

        let connection = WebClient::builder()
            .auth(WebAuth::Bearer(self.app_key.clone()))
            .host_port(host_port)
            .api_root(self.activity_url.path())
            .build()?;
        let client = std::sync::Arc::new(connection);

        Ok(ya_client::activity::RequestorControlApiClient::new(client))
    }
}

async fn process_offer(
    requestor_api: RequestorApi,
    provider_id: String,
    offer: Proposal,
) -> Result<String, Box<dyn std::error::Error>> {
    let agreement_id = offer.id.clone();
    let agreement = Agreement::new(agreement_id.clone(), "2021-01-01".to_string());
    let _ack = requestor_api.create_agreement(&agreement).await?;
    log::info!("confirm agreement = {}", agreement_id);
    requestor_api.confirm_agreement(&agreement_id).await?;
    log::info!("wait for agreement = {}", agreement_id);
    requestor_api.wait_for_approval(&agreement_id).await?;

    Ok(agreement_id)
}

async fn spawn_workers(
    requestor_api: RequestorApi,
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
                RequestorEvent::OfferEvent {
                    provider_id,
                    offer: Some(offer),
                } => {
                    let mut tx = tx.clone();
                    let requestor_api = requestor_api.clone();
                    Arbiter::spawn(async move {
                        let agreement_id = process_offer(requestor_api, provider_id, offer)
                            .await
                            .unwrap();
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

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::try_init()?;

    let settings = AppSettings::from_args();

    let node_name = "test1";

    let demand = Demand {
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
    };
    //(golem.runtime.wasm.wasi.version@v=*)

    let client = settings.market_api()?;
    let subscription_id = client.requestor().subscribe(&demand).await?;

    eprintln!("sub_id={}", subscription_id);

    {
        let requestor_api = client.requestor().clone();
        let subscription_id = subscription_id.clone();
        Arbiter::spawn(async move {
            tokio::signal::ctrl_c().await.unwrap();
            requestor_api.unsubscribe(&subscription_id).await.unwrap();
        })
    }
    let requestor_api = client.requestor().clone();
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

    client.requestor().unsubscribe(&subscription_id).await?;
    Ok(())
}
