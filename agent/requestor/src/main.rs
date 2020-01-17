use ya_client::{
    market::{ApiClient, ProviderApi, RequestorApi},
    web::WebClient,
    Error, Result,
};
use ya_model::market::{Agreement, Demand, Offer, Proposal, ProviderEvent, RequestorEvent};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let client = ApiClient::new(WebClient::builder())?;

    let demand = Demand {
        properties: json!{{
            "golem": {
                "node": {
                    "id": ""
                }
            }
        }},
        constraints: r#"



        "#.to_string()
    };

    let subscription_id = client.requestor().subscribe(&demand).await?;

    eprintln!("sub_id={}", subscription_id);

    Ok(())
}
