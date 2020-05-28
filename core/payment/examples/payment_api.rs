use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use ethkey::{EthAccount, Password};
use structopt::StructOpt;
use ya_client_model::market;
use ya_client_model::payment::PAYMENT_API_PATH;

use ya_payment::processor::PaymentProcessor;
use ya_payment::{migrations, utils};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::auth::dummy::DummyAuth;
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::rest_api_addr;

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(long, default_value = "provider.key")]
    provider_key_path: String,
    #[structopt(long, default_value = "")]
    provider_pass: String,
    #[structopt(long, default_value = "requestor.key")]
    requestor_key_path: String,
    #[structopt(long, default_value = "")]
    requestor_pass: String,
    #[structopt(long, default_value = "agreement_id")]
    agreement_id: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var(
        "RUST_LOG",
        "debug,tokio_core=info,tokio_reactor=info,hyper=info",
    );
    env_logger::init();
    dotenv::dotenv().expect("Failed to read .env file");

    let args: Args = Args::from_args();

    let provider_pass: Password = args.provider_pass.clone().into();
    let requestor_pass: Password = args.requestor_pass.clone().into();
    let provider_account = EthAccount::load_or_generate(&args.provider_key_path, provider_pass)?;
    let requestor_account = EthAccount::load_or_generate(&args.requestor_key_path, requestor_pass)?;
    let provider_id = provider_account.address().to_string();
    let requestor_id = requestor_account.address().to_string();
    log::info!(
        "Provider ID: {}\nRequestor ID: {}",
        provider_id,
        requestor_id
    );

    let database_url = "file:yagna.db";
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(migrations::run_with_output)?;

    ya_sb_router::bind_gsb_router(None).await?;
    let processor = PaymentProcessor::new(db.clone());
    ya_payment::service::bind_service(&db, processor);

    let agreement = market::Agreement {
        agreement_id: args.agreement_id.clone(),
        demand: market::Demand {
            properties: Default::default(),
            constraints: "".to_string(),
            demand_id: None,
            requestor_id: Some(requestor_id.clone()),
        },
        offer: market::Offer {
            properties: Default::default(),
            constraints: "".to_string(),
            offer_id: None,
            provider_id: Some(provider_id.clone()),
        },
        valid_to: Utc::now(),
        approved_date: None,
        state: market::agreement::State::Proposal,
        proposed_signature: None,
        approved_signature: None,
        committed_signature: None,
    };
    utils::fake_get_agreement(args.agreement_id.clone(), agreement);
    utils::provider::fake_get_agreement_id(args.agreement_id.clone());

    let provider_id = provider_id.parse()?;
    let requestor_id = requestor_id.parse()?;
    ya_net::bind_remote(provider_id, vec![provider_id, requestor_id]).await?;

    HttpServer::new(move || {
        let provider_identity = Identity {
            identity: provider_id,
            name: "".to_string(),
            role: "".to_string(),
        };
        let requestor_identity = Identity {
            identity: requestor_id,
            name: "".to_string(),
            role: "".to_string(),
        };

        let provider_scope =
            ya_payment::api::provider_scope().wrap(DummyAuth::new(provider_identity));
        let requestor_scope =
            ya_payment::api::requestor_scope().wrap(DummyAuth::new(requestor_identity));
        let payment_service = Scope::new(PAYMENT_API_PATH)
            .data(db.clone())
            .service(provider_scope)
            .service(requestor_scope);
        App::new()
            .wrap(middleware::Logger::default())
            .service(payment_service)
    })
    .bind(rest_api_addr())?
    .run()
    .await?;

    Ok(())
}
