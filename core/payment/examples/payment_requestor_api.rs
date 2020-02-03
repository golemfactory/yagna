use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use std::str::FromStr;
use ya_core_model::ethaddr::NodeId;
use ya_model::market;
use ya_model::payment::PAYMENT_API_PATH;
use ya_payment::{migrations, utils};
use ya_persistence::executor::DbExecutor;
use ya_service_api::constants::{YAGNA_BUS_ADDR, YAGNA_HTTP_ADDR};
use ya_service_api_web::middleware::auth::dummy::DummyAuth;
use ya_service_api_web::middleware::Identity;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    migrations::run_with_output(&db.conn()?, &mut std::io::stdout())?;

    ya_sb_router::bind_router(*YAGNA_BUS_ADDR).await?;
    ya_payment::service::bind_service(&db);

    let requestor_id = "0x9a3632f8c195d6c04b67499e264b2dfc8af40103";
    let provider_id = "0xd39a168f0480b8502c2531b2ffd8588c592d713a";

    let net_host = ya_net::resolve_default()?;
    ya_net::bind_remote(&net_host, &requestor_id).await?;

    let agreement_id = "agreement_id";
    let agreement = market::Agreement {
        agreement_id: agreement_id.to_owned(),
        demand: market::Demand {
            properties: Default::default(),
            constraints: "".to_string(),
            demand_id: None,
            requestor_id: Some(requestor_id.to_owned()),
        },
        offer: market::Offer {
            properties: Default::default(),
            constraints: "".to_string(),
            offer_id: None,
            provider_id: Some(provider_id.to_owned()),
        },
        valid_to: Utc::now(),
        approved_date: None,
        state: market::agreement::State::Proposal,
        proposed_signature: None,
        approved_signature: None,
        committed_signature: None,
    };
    utils::fake_get_agreement(agreement_id.to_owned(), agreement.clone());

    let requestor_identity = Identity {
        identity: NodeId::from_str(requestor_id).unwrap(),
        name: "".to_string(),
        role: "".to_string(),
    };

    HttpServer::new(move || {
        let payment_service = Scope::new(PAYMENT_API_PATH)
            .data(db.clone())
            .service(ya_payment::api::requestor_scope());

        App::new()
            .wrap(middleware::Logger::default())
            .wrap(DummyAuth::new(requestor_identity.clone()))
            .service(payment_service)
    })
    .bind(*YAGNA_HTTP_ADDR)?
    .run()
    .await?;

    Ok(())
}
