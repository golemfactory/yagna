use structopt::StructOpt;
use ya_client::web::WebAuth;
use ya_client::{
    market::{ApiClient, ProviderApi, RequestorApi},
    web::WebClient,
};
use ya_model::market::{Agreement, Demand, Offer, Proposal, ProviderEvent, RequestorEvent};

#[derive(StructOpt)]
struct AppSettings {
    /// Authorization token to server
    #[structopt(long = "app-key", env = "YAGNA_APPKEY")]
    app_key: String,

    ///
    #[structopt(long = "market-url", env = "YAGNA_MARKET_URL")]
    market_url: String,

    ///
    #[structopt(long = "activity-url", env = "YAGNA_ACTIVITY_URL")]
    activity_url: String,
}

async fn spawn_workers(
    client: ApiClient,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let events = client
        .requestor()
        .collect(&subscription_id, Some(10), Some(5))
        .await;

    Ok(())
}

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv()?;

    let connection = WebClient::builder()
        .auth(WebAuth::Bearer("3d1724b4682642bfa6686ebc6858d5a6".into()))
        .host_port("10.30.10.202:5001");
    let client = ApiClient::new(connection)?;

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
            (golem.inf.runtime.wasm.wasi.version@v=*)
        )"#
        .to_string(),
    };

    let subscription_id = client.requestor().subscribe(&demand).await?;

    eprintln!("sub_id={}", subscription_id);

    Ok(())
}
