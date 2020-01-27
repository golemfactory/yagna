use actix_rt::Arbiter;
use std::future::Future;
use structopt::StructOpt;
use url::Url;
use ya_client::web::WebAuth;
use ya_client::{
    market::{ApiClient, ProviderApi, RequestorApi},
    web::WebClient,
};
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
}

async fn spawn_workers(
    requestor_api: RequestorApi,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let events = requestor_api
        .collect(&subscription_id, Some(10), Some(5))
        .await;

    eprintln!("events={:?}", events);

    Ok(())
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

    spawn_workers(requestor_api.clone(), &subscription_id).await?;

    client.requestor().unsubscribe(&subscription_id).await;
    Ok(())
}
