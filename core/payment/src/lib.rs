#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
use crate::processor::PaymentProcessor;
use ya_payment_driver::PaymentDriver;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::*;

#[macro_use]
extern crate diesel;

pub mod api;
mod cli;
pub mod dao;
pub mod error;
pub mod models;
pub mod processor;
pub mod schema;
pub mod service;
pub mod utils;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const GNT_RINKEBY_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";

const ETH_FAUCET_ADDRESS: &str = "http://188.165.227.180:4000/donate";
const GNT_FAUCET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";

#[cfg(feature = "dummy-driver")]
fn payment_driver_factory(db: &DbExecutor) -> anyhow::Result<impl PaymentDriver> {
    use ya_payment_driver::DummyDriver;

    Ok(DummyDriver::new())
}

#[cfg(feature = "gnt-driver")]
fn payment_driver_factory(db: &DbExecutor) -> anyhow::Result<impl PaymentDriver> {
    use ya_payment_driver::{Chain, GntDriver};

    let gnt_contract_address: ethereum_types::Address = GNT_RINKEBY_CONTRACT.parse()?;
    let gnt_faucet_address: ethereum_types::Address = GNT_FAUCET_CONTRACT.parse()?;

    Ok(GntDriver::new(
        Chain::Rinkeby,
        GETH_ADDRESS,
        gnt_contract_address,
        ETH_FAUCET_ADDRESS,
        gnt_faucet_address,
        db.clone(),
    )?)
}

pub struct PaymentService;

impl Service for PaymentService {
    type Cli = cli::PaymentCli;
}

impl PaymentService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        db.apply_migration(migrations::run_with_output)?;
        let driver = payment_driver_factory(&db)?;
        let processor = PaymentProcessor::new(driver, db.clone());
        self::service::bind_service(&db, processor);
        Ok(())
    }

    pub fn rest(db: &DbExecutor) -> actix_web::Scope {
        api::web_scope(db)
    }
}
