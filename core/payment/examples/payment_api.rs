#![allow(clippy::type_complexity)]

use actix_web::web::Data;
use actix_web::{middleware, App, HttpServer, Scope};
use chrono::Utc;
use ethsign::keyfile::Bytes;
use ethsign::{KeyFile, Protected, SecretKey};
use futures::Future;
use rand::Rng;
use ya_payment::service::BindOptions;

use std::convert::TryInto;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use structopt::StructOpt;
use ya_client_model::market;
use ya_client_model::payment::PAYMENT_API_PATH;
use ya_client_model::NodeId;
use ya_core_model::driver::{driver_bus_id, AccountMode, Fund, Init};
use ya_core_model::identity;
use ya_dummy_driver as dummy;
use ya_erc20_driver as erc20;
use ya_erc20next_driver as erc20next;
use ya_net::Config;
use ya_payment::processor::PaymentProcessor;
use ya_payment::{migrations, utils, PaymentService};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::auth::dummy::DummyAuth;
use ya_service_api_web::middleware::Identity;
use ya_service_api_web::rest_api_addr;
use ya_service_api_web::scope::ExtendableScope;
use ya_service_bus::typed as bus;

#[derive(Clone, Debug, StructOpt)]
enum Driver {
    Dummy,
    Erc20,
    Erc20next,
}

impl FromStr for Driver {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "dummy" => Ok(Driver::Dummy),
            "erc20" => Ok(Driver::Erc20),
            "erc20next" => Ok(Driver::Erc20next),
            s => Err(anyhow::Error::msg(format!("Invalid driver: {}", s))),
        }
    }
}

impl std::fmt::Display for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Driver::Dummy => write!(f, "dummy"),
            Driver::Erc20 => write!(f, "erc20"),
            Driver::Erc20next => write!(f, "erc20next"),
        }
    }
}

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(long, default_value = "dummy")]
    driver: Driver,
    #[structopt(long)]
    network: Option<String>,
    #[structopt(long, default_value = "dummy-glm")]
    platform: String,
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
    #[structopt(long)]
    app_session_id: Option<String>,
}

pub async fn start_dummy_driver() -> anyhow::Result<()> {
    dummy::PaymentDriverService::gsb(&()).await?;
    Ok(())
}

pub async fn start_erc20_driver(
    db: &DbExecutor,
    requestor_account: SecretKey,
) -> anyhow::Result<()> {
    let requestor = NodeId::from(requestor_account.public().address().as_ref());
    fake_list_identities(vec![requestor]);
    fake_subscribe_to_events();

    erc20::PaymentDriverService::gsb(db).await?;

    let requestor_sign_tx = get_sign_tx(requestor_account);
    fake_sign_tx(Box::new(requestor_sign_tx));
    Ok(())
}

pub async fn start_erc20_next_driver(
    db: &DbExecutor,
    path: PathBuf,
    requestor_account: SecretKey,
) -> anyhow::Result<()> {
    let requestor = NodeId::from(requestor_account.public().address().as_ref());
    fake_list_identities(vec![requestor]);
    fake_subscribe_to_events();

    erc20next::PaymentDriverService::gsb(db, path).await?;

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

fn get_sign_tx(account: SecretKey) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
    let account: Arc<SecretKey> = account.into();
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

const KEY_ITERATIONS: u32 = 2;
const KEYSTORE_VERSION: u64 = 3;

fn load_or_generate(path: &str, password: Protected) -> SecretKey {
    log::debug!("load_or_generate({}, {:?})", path, &password);
    if let Ok(file) = std::fs::File::open(path) {
        // Broken keyfile should panic
        let key: KeyFile = serde_json::from_reader(file).unwrap();
        // Invalid password should panic
        let secret = key.to_secret_key(&password).unwrap();
        log::info!("Loaded key. path={}", path);
        return secret;
    }
    // File does not exist, create new key
    let random_bytes: [u8; 32] = rand::thread_rng().gen();
    let secret = SecretKey::from_raw(random_bytes.as_ref()).unwrap();
    let key_file = KeyFile {
        id: format!("{}", uuid::Uuid::new_v4()),
        version: KEYSTORE_VERSION,
        crypto: secret.to_crypto(&password, KEY_ITERATIONS).unwrap(),
        address: Some(Bytes(secret.public().address().to_vec())),
    };
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(serde_json::to_string_pretty(&key_file).unwrap().as_ref())
        .unwrap();
    log::info!("Generated new key. path={}", path);
    secret
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var(
            "RUST_LOG",
            "debug,tokio_core=info,tokio_reactor=info,hyper=info,reqwest=info",
        );
    }
    env_logger::init();
    dotenv::dotenv().expect("Failed to read .env file");

    let args: Args = Args::from_args();

    let provider_pass: Protected = args.provider_pass.clone().into();
    let provider_account = load_or_generate(&args.provider_key_path, provider_pass);
    let provider_id = format!("0x{}", hex::encode(provider_account.public().address()));
    let provider_addr = args
        .provider_addr
        .unwrap_or_else(|| provider_id.clone())
        .to_lowercase();
    let provider_pub_key = provider_account.public();

    let requestor_pass: Protected = args.requestor_pass.clone().into();
    let requestor_account = load_or_generate(&args.requestor_key_path, requestor_pass);
    let requestor_id = format!("0x{}", hex::encode(requestor_account.public().address()));
    let requestor_addr = args
        .requestor_addr
        .unwrap_or_else(|| requestor_id.clone())
        .to_lowercase();
    let requestor_pub_key = requestor_account.public();

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

    log::debug!("bind_gsb_router()");

    let processor = PaymentProcessor::new(db.clone());
    ya_payment::service::bind_service(&db, processor, BindOptions::default().run_sync_job(false));
    log::debug!("bind_service()");

    let driver_name = match args.driver {
        Driver::Dummy => {
            start_dummy_driver().await?;
            dummy::DRIVER_NAME
        }
        Driver::Erc20 => {
            start_erc20_driver(&db, requestor_account).await?;
            erc20::DRIVER_NAME
        }
        Driver::Erc20next => {
            start_erc20_next_driver(&db, "./".into(), requestor_account).await?;
            erc20next::DRIVER_NAME
        }
    };
    bus::service(driver_bus_id(driver_name))
        .call(Init::new(
            provider_addr.clone(),
            args.network.clone(),
            None,
            AccountMode::RECV,
        ))
        .await??;

    bus::service(driver_bus_id(driver_name))
        .call(Fund::new(
            requestor_addr.clone(),
            args.network.clone(),
            None,
        ))
        .await??;
    bus::service(driver_bus_id(driver_name))
        .call(Init::new(
            requestor_addr.clone(),
            args.network.clone(),
            None,
            AccountMode::SEND,
        ))
        .await??;

    let address_property = format!("platform.{}.address", args.platform);
    let demand_properties = serde_json::json!({
        "golem.com.payment": {
            "chosen-platform": &args.platform,
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

    log::info!("start agreement...");

    let agreement = market::Agreement {
        agreement_id: args.agreement_id.clone(),
        demand: market::Demand {
            properties: demand_properties,
            constraints: "".to_string(),
            demand_id: "".to_string(),
            requestor_id: requestor_id.parse().unwrap(),
            timestamp: Utc::now(),
        },
        offer: market::Offer {
            properties: offer_properties,
            constraints: "".to_string(),
            offer_id: "".to_string(),
            provider_id: provider_id.parse().unwrap(),
            timestamp: Utc::now(),
        },
        valid_to: Utc::now(),
        approved_date: None,
        state: market::agreement::State::Proposal,
        timestamp: Utc::now(),
        app_session_id: args.app_session_id,
        proposed_signature: None,
        approved_signature: None,
        committed_signature: None,
    };
    utils::fake_get_agreement(args.agreement_id.clone(), agreement);
    utils::provider::fake_get_agreement_id(args.agreement_id.clone());

    bus::bind(identity::BUS_ID, {
        let provider_key = provider_pub_key.clone();
        let requestor_key = requestor_pub_key.clone();
        move |msg: identity::GetPubKey| {
            let node_id: &[u8; 20] = msg.0.as_ref();
            let pub_key =
                if node_id == provider_key.address() {
                    Some(provider_key.bytes())
                } else if node_id == requestor_key.address() {
                    Some(requestor_key.bytes())
                } else {
                    None
                }
                .map(|bytes| bytes.into_iter().cloned().collect::<Vec<_>>())
                .ok_or(identity::Error::NodeNotFound(Box::new(msg.0)));
            async move {
                pub_key
            }
        }
    });

    let provider_id = provider_id.parse()?;
    let requestor_id = requestor_id.parse()?;
    log::info!("bind remote...");

    ya_net::hybrid::start_network(
        Arc::new(Config::from_env()?),
        provider_id,
        vec![provider_id, requestor_id],
    )
    .await?;

    log::info!("get_rest_addr...");
    let rest_addr = rest_api_addr();
    log::info!("Starting http server on port {}", rest_addr);

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

        let provider_api_scope = Scope::new(&format!("provider/{}", PAYMENT_API_PATH))
            .app_data(Data::new(db.clone()))
            .extend(ya_payment::api::api_scope)
            .wrap(DummyAuth::new(provider_identity));
        let requestor_api_scope = Scope::new(&format!("requestor/{}", PAYMENT_API_PATH))
            .app_data(Data::new(db.clone()))
            .extend(ya_payment::api::api_scope)
            .wrap(DummyAuth::new(requestor_identity));
        App::new()
            .wrap(middleware::Logger::default())
            .service(provider_api_scope)
            .service(requestor_api_scope)
    })
    .bind(rest_addr)?
    .run()
    .await?;

    PaymentService::shut_down().await;

    Ok(())
}
