use structopt::StructOpt;
use ya_client::web::WebAuth;
use ya_client::{
    market::{ApiClient, ProviderApi, RequestorApi},
    web::WebClient,
};
use ya_model::market::{Agreement, Demand, Offer, Proposal, ProviderEvent, RequestorEvent};

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = WebClient::builder()
        .auth(WebAuth::Bearer("3d1724b4682642bfa6686ebc6858d5a6".into()))
        .host_port("127.0.0.1:5001");
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
            (golem.node.inf.mem.gib>0.5)
            (golem.node.inf.storage.gib>1)
            (golem.node.inf.runtime.wasm.wasi.version@v=*)
        )"#
        .to_string(),
    };

    let subscription_id = client.requestor().subscribe(&demand).await?;

    eprintln!("sub_id={}", subscription_id);

    Ok(())
}
