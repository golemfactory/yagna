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

#[cfg(feature = "dummy-driver")]
mod driver {
    use super::{DbExecutor, EthAccount};
    use ya_payment_driver::PaymentDriverService;

    pub async fn start(
        db: &DbExecutor,
        _provider_account: Box<EthAccount>,
        _requestor_account: Box<EthAccount>,
    ) -> anyhow::Result<()> {
        PaymentDriverService::gsb(db).await?;
        Ok(())
    }
}

#[cfg(feature = "gnt-driver")]
mod driver {
    use super::{DbExecutor, EthAccount};
    use futures::Future;
    use std::convert::TryInto;
    use std::pin::Pin;
    use std::sync::Arc;
    use ya_core_model::identity;
    use ya_payment_driver::PaymentDriverService;
    use ya_service_bus::typed as bus;

    pub async fn start(
        db: &DbExecutor,
        provider_account: Box<EthAccount>,
        requestor_account: Box<EthAccount>,
    ) -> anyhow::Result<()> {
        let provider_sign_tx = get_sign_tx(provider_account);
        let requestor_sign_tx = get_sign_tx(requestor_account);

        PaymentDriverService::gsb(db).await?;

        fake_sign_tx(Box::new(provider_sign_tx));
        fake_sign_tx(Box::new(requestor_sign_tx));
        Ok(())
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
        let sign_tx: Arc<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>> =
            sign_tx.into();
        bus::bind(identity::BUS_ID, move |msg: identity::Sign| {
            let sign_tx = sign_tx.clone();
            let msg = msg.payload;
            async move { Ok(sign_tx(msg).await) }
        });
    }
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

    let database_url = "file:payment.db";
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(migrations::run_with_output)?;

    ya_sb_router::bind_gsb_router(None).await?;
    driver::start(&db, provider_account, requestor_account).await?;
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
