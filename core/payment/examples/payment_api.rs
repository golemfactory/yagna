use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use ethkey::{EthAccount, Password};
use futures::Future;
use serde_json;
use std::convert::TryInto;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use structopt::StructOpt;
use ya_client_model::market;
use ya_client_model::payment::PAYMENT_API_PATH;
use ya_client_model::NodeId;
use ya_core_model::driver::{driver_bus_id, AccountMode, Init};
use ya_core_model::identity;
use ya_dummy_driver as dummy;
use ya_gnt_driver as gnt;
use ya_payment::processor::PaymentProcessor;
use ya_payment::{migrations, utils};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::auth::dummy::DummyAuth;
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::rest_api_addr;
use ya_service_bus::typed as bus;

#[derive(Clone, Debug, StructOpt)]
enum Driver {
    Dummy,
    Ngnt,
}

impl FromStr for Driver {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "dummy" => Ok(Driver::Dummy),
            "ngnt" => Ok(Driver::Ngnt),
            s => Err(anyhow::Error::msg(format!("Invalid driver: {}", s))),
        }
    }
}

impl std::fmt::Display for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Driver::Dummy => write!(f, "dummy"),
            Driver::Ngnt => write!(f, "ngnt"),
        }
    }
}

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(long, default_value = "dummy")]
    driver: Driver,
    #[structopt(long, default_value = "provider.key")]
    provider_key_path: String,
    #[structopt(long, default_value = "")]
    provider_pass: String,
    #[structopt(long)]
    provider_addr: Option<String>,
    #[structopt(long, default_value = "requestor.key")]
    requestor_key_path: String,
    #[structopt(long, default_value = "")]
    requestor_pass: String,
    #[structopt(long)]
    requestor_addr: Option<String>,
    #[structopt(long, default_value = "agreement_id")]
    agreement_id: String,
}

pub async fn start_dummy_driver() -> anyhow::Result<()> {
    dummy::PaymentDriverService::gsb(&()).await?;
    Ok(())
}

pub async fn start_gnt_driver(
    db: &DbExecutor,
    requestor_account: Box<EthAccount>,
) -> anyhow::Result<()> {
    let requestor = NodeId::from(requestor_account.address().as_ref());
    fake_list_identities(vec![requestor]);
    fake_subscribe_to_events();

    gnt::PaymentDriverService::gsb(db).await?;

    let requestor_sign_tx = get_sign_tx(requestor_account);
    fake_sign_tx(Box::new(requestor_sign_tx));
    Ok(())
}

fn fake_list_identities(identities: Vec<NodeId>) {
    bus::bind(identity::BUS_ID, move |_msg: identity::List| {
        let ids = identities.clone();
        let mut accounts: Vec<identity::IdentityInfo> = vec![];
        for id in ids {
            accounts.push(identity::IdentityInfo {
                alias: None,
                node_id: id,
                is_default: false,
                is_locked: false,
            });
        }
        async move { Ok(accounts) }
    });
}

fn fake_subscribe_to_events() {
    bus::bind(
        identity::BUS_ID,
        move |_msg: identity::Subscribe| async move { Ok(identity::Ack {}) },
    );
}

fn get_sign_tx(
    account: Box<EthAccount>,
) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
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

fn fake_sign_tx(sign_tx: Box<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>>) {
    let sign_tx: Arc<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>> = sign_tx.into();
    bus::bind(identity::BUS_ID, move |msg: identity::Sign| {
        let sign_tx = sign_tx.clone();
        let msg = msg.payload;
        async move { Ok(sign_tx(msg).await) }
    });
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
    let provider_account = EthAccount::load_or_generate(&args.provider_key_path, provider_pass)?;
    let provider_id = provider_account.address().to_string();
    let provider_addr = args.provider_addr.unwrap_or(provider_id.clone());

    let requestor_pass: Password = args.requestor_pass.clone().into();
    let requestor_account = EthAccount::load_or_generate(&args.requestor_key_path, requestor_pass)?;
    let requestor_id = requestor_account.address().to_string();
    let requestor_addr = args.requestor_addr.unwrap_or(requestor_id.clone());

    log::info!(
        "Provider ID: {}\nProvider address: {}\nRequestor ID: {}\nRequestor address: {}",
        provider_id,
        provider_addr,
        requestor_id,
        requestor_addr,
    );

    let database_url = "file:payment.db";
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(migrations::run_with_output)?;

    ya_sb_router::bind_gsb_router(None).await?;

    let (driver_name, platform) = match args.driver {
        Driver::Dummy => {
            start_dummy_driver().await?;
            (dummy::DRIVER_NAME, dummy::PLATFORM_NAME)
        }
        Driver::Ngnt => {
            start_gnt_driver(&db, requestor_account).await?;
            (gnt::DRIVER_NAME, gnt::PLATFORM_NAME)
        }
    };

    let processor = PaymentProcessor::new(db.clone());
    ya_payment::service::bind_service(&db, processor);

    bus::service(driver_bus_id(driver_name))
        .call(Init::new(provider_id.clone(), AccountMode::RECV))
        .await??;
    bus::service(driver_bus_id(driver_name))
        .call(Init::new(requestor_id.clone(), AccountMode::SEND))
        .await??;

    let address_property = format!("platform.{}.address", platform);
    let demand_properties = serde_json::json!({
        "golem.com.payment": {
            "chosen-platform": &platform,
            &address_property: &requestor_addr,
        }
    });
    log::info!(
        "Demand properties: {}",
        serde_json::to_string(&demand_properties)?
    );
    let offer_properties = serde_json::json!({
        "golem.com.payment": {
            &address_property: &provider_addr,
        }
    });
    log::info!(
        "Offer properties: {}",
        serde_json::to_string(&offer_properties)?
    );

    let agreement = market::Agreement {
        agreement_id: args.agreement_id.clone(),
        demand: market::Demand {
            properties: demand_properties,
            constraints: "".to_string(),
            demand_id: None,
            requestor_id: Some(requestor_id.clone()),
        },
        offer: market::Offer {
            properties: offer_properties,
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
