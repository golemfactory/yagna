use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use ethereum_types::{Address, H160};
use ethkey::{EthAccount, Password};
use futures::Future;
use lazy_static::lazy_static;
use std::convert::TryInto;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use structopt::StructOpt;
use ya_core_model::ethaddr::NodeId;
use ya_model::market;
use ya_model::payment::PAYMENT_API_PATH;
use ya_payment::processor::PaymentProcessor;
use ya_payment::utils::fake_sign_tx;
use ya_payment::{migrations, utils};
use ya_payment_driver::{AccountMode, Chain, DummyDriver, GntDriver, PaymentDriver, SignTx};
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

lazy_static! {
    pub static ref GETH_ADDR: String =
        std::env::var("GETH_ADDR").unwrap_or("http://188.165.227.180:55555".into());
    pub static ref ETH_FAUCET_ADDR: String =
        std::env::var("ETH_FAUCET_ADDR").unwrap_or("http://188.165.227.180:4000/donate".into());
    pub static ref GNT_CONTRACT_ADDR: Address = std::env::var("GNT_CONTRACT_ADDR")
        .unwrap_or("924442A66cFd812308791872C4B242440c108E19".into())
        .parse()
        .unwrap();
    pub static ref GNT_FAUCET_ADDR: Address = std::env::var("GNT_FAUCET_ADDR")
        .unwrap_or("77b6145E853dfA80E8755a4e824c4F510ac6692e".into())
        .parse()
        .unwrap();
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
    address: Address,
    sign_tx: SignTx<'_>,
    command: Command,
) -> anyhow::Result<GntDriver> {
    let driver = GntDriver::new(
        Chain::Rinkeby,
        &*GETH_ADDR,
        *GNT_CONTRACT_ADDR,
        (*ETH_FAUCET_ADDR).to_string(),
        *GNT_FAUCET_ADDR,
        db.clone(),
    )?;

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
    let address = H160::from_slice(account.address().as_ref());
    log::info!("Node ID: {}", node_id);
    let sign_tx = get_sign_tx(account);

    let database_url = format!("file:{}?mode=memory&cache=shared", &node_id);
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(migrations::run_with_output)?;

    ya_sb_router::bind_router(*YAGNA_BUS_ADDR).await?;
    let processor = match &args.driver {
        Driver::Dummy => PaymentProcessor::new(DummyDriver::new(), db.clone()),
        Driver::Gnt => PaymentProcessor::new(
            get_gnt_driver(&db, address, &sign_tx, args.command.clone()).await?,
            db.clone(),
        ),
    };
    ya_payment::service::bind_service(&db, processor);
    fake_sign_tx(Box::new(sign_tx));

    let net_host = ya_net::resolve_default()?;
    ya_net::bind_remote(&net_host, &node_id).await?;

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
