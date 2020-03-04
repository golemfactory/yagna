use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use std::str::FromStr;
use structopt::StructOpt;
use ya_core_model::ethaddr::NodeId;
use ya_model::market;
use ya_model::payment::PAYMENT_API_PATH;
use ya_payment::processor::PaymentProcessor;
use ya_payment::{migrations, utils};
use ya_payment_driver::DummyDriver;
use ya_persistence::executor::DbExecutor;
use ya_service_api::constants::{YAGNA_BUS_ADDR, YAGNA_HTTP_ADDR};
use ya_service_api_web::middleware::auth::dummy::DummyAuth;
use ya_service_api_web::middleware::Identity;

#[derive(Clone, Debug, StructOpt)]
enum Command {
    Provider,
    Requestor,
}

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(subcommand)]
    command: Command,
    #[structopt(long, default_value = "0x9a3632f8c195d6c04b67499e264b2dfc8af40103")]
    requestor_id: String,
    #[structopt(long, default_value = "0xd39a168f0480b8502c2531b2ffd8588c592d713a")]
    provider_id: String,
    #[structopt(long, default_value = "agreement_id")]
    agreement_id: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let args: Args = Args::from_args();
    let node_id = match &args.command {
        Command::Provider => args.provider_id.clone(),
        Command::Requestor => args.requestor_id.clone(),
    };

    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let database_url = format!("file:{}?mode=memory&cache=shared", &node_id);
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(migrations::run_with_output)?;

    ya_sb_router::bind_router(*YAGNA_BUS_ADDR).await?;
    let driver = DummyDriver::new();
    let processor = PaymentProcessor::new(driver, db.clone());
    ya_payment::service::bind_service(&db, processor);

    let net_host = ya_net::resolve_default()?;
    ya_net::bind_remote(&net_host, &node_id).await?;

    let agreement = market::Agreement {
        agreement_id: args.agreement_id.clone(),
        demand: market::Demand {
            properties: Default::default(),
            constraints: "".to_string(),
            demand_id: None,
            requestor_id: Some(args.requestor_id.clone()),
        },
        offer: market::Offer {
            properties: Default::default(),
            constraints: "".to_string(),
            offer_id: None,
            provider_id: Some(args.provider_id.clone()),
        },
        valid_to: Utc::now(),
        approved_date: None,
        state: market::agreement::State::Proposal,
        proposed_signature: None,
        approved_signature: None,
        committed_signature: None,
    };
    utils::fake_get_agreement(args.agreement_id.clone(), agreement);

    let identity = Identity {
        identity: NodeId::from_str(&node_id).unwrap(),
        name: "".to_string(),
        role: "".to_string(),
    };

    HttpServer::new(move || {
        let scope = match &args.command {
            Command::Provider => ya_payment::api::provider_scope(),
            Command::Requestor => ya_payment::api::requestor_scope(),
        };
        let payment_service = Scope::new(PAYMENT_API_PATH).data(db.clone()).service(scope);
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(DummyAuth::new(identity.clone()))
            .service(payment_service)
    })
    .bind(*YAGNA_HTTP_ADDR)?
    .run()
    .await?;

    Ok(())
}
