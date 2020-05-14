use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use ethkey::{EthAccount, Password};
use futures::Future;
use std::convert::TryInto;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use structopt::StructOpt;
use ya_client_model::market;
use ya_client_model::payment::PAYMENT_API_PATH;

use ya_payment::processor::PaymentProcessor;
use ya_payment::utils::fake_sign_tx;
use ya_payment::{migrations, utils};
use ya_payment_driver::{AccountMode, DummyDriver, GntDriver, PaymentDriver, SignTx};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::auth::dummy::DummyAuth;
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::rest_api_addr;

#[derive(Clone, Debug, StructOpt)]
enum Command {
    Provider,
    Requestor,
}

#[derive(Clone, Debug, StructOpt)]
enum Driver {
    Dummy,
    Gnt,
}

impl FromStr for Driver {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "dummy" => Ok(Driver::Dummy),
            "gnt" => Ok(Driver::Gnt),
            s => Err(anyhow::Error::msg(format!("Invalid driver: {}", s))),
        }
    }
}

impl std::fmt::Display for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Driver::Dummy => write!(f, "dummy"),
            Driver::Gnt => write!(f, "gnt"),
        }
    }
}

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(subcommand)]
    command: Command,
    #[structopt(long, default_value = "dummy")]
    driver: Driver,
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

async fn get_gnt_driver(
    db: &DbExecutor,
    address: &str,
    sign_tx: SignTx<'_>,
    command: Command,
) -> anyhow::Result<GntDriver> {
    let driver = GntDriver::new(db.clone()).await?;

    let mode = match command {
        Command::Provider => AccountMode::RECV,
        Command::Requestor => AccountMode::SEND,
    };
    driver.init(mode, address, sign_tx).await?;
    Ok(driver)
}

fn get_sign_tx(
    account: Box<EthAccount>,
) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
    // let account: Arc<EthAccount> = EthAccount::load_or_generate(key_path, password).unwrap().into();
    let account: Arc<EthAccount> = account.into();
    move |msg| {
        let account = account.clone();
        let fut = async move {
            let msg: [u8; 32] = msg.as_slice().try_into().unwrap();
            let signature = account.sign(&msg).unwrap();
            let mut v = Vec::with_capacity(65);
            v.push(signature.v);
            v.extend_from_slice(&signature.r);
            v.extend_from_slice(&signature.s);
            v
        };
        Box::pin(fut)
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    dotenv::dotenv().expect("Failed to read .env file");

    let args: Args = Args::from_args();

    let provider_pass: Password = args.provider_pass.clone().into();
    let requestor_pass: Password = args.requestor_pass.clone().into();
    let provider_account = EthAccount::load_or_generate(&args.provider_key_path, provider_pass)?;
    let requestor_account = EthAccount::load_or_generate(&args.requestor_key_path, requestor_pass)?;
    let provider_id = provider_account.address().to_string();
    let requestor_id = requestor_account.address().to_string();
    let (account, node_id) = match &args.command {
        Command::Provider => (provider_account, provider_id.clone()),
        Command::Requestor => (requestor_account, requestor_id.clone()),
    };
    let address = hex::encode(account.address());
    log::info!("Node ID: {}", node_id);
    let sign_tx = get_sign_tx(account);

    let database_url = format!("file:{}.db", &node_id);
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(migrations::run_with_output)?;

    ya_sb_router::bind_gsb_router(None).await?;
    let processor = match &args.driver {
        Driver::Dummy => PaymentProcessor::new(DummyDriver::new(), db.clone()),
        Driver::Gnt => PaymentProcessor::new(
            get_gnt_driver(&db, address.as_str(), &sign_tx, args.command.clone()).await?,
            db.clone(),
        ),
    };
    ya_payment::service::bind_service(&db, processor);
    fake_sign_tx(Box::new(sign_tx));

    let node_id = node_id.parse()?;
    ya_net::bind_remote(node_id, vec![node_id]).await?;

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

    let identity = Identity {
        identity: node_id,
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
    .bind(rest_api_addr())?
    .run()
    .await?;

    Ok(())
}
