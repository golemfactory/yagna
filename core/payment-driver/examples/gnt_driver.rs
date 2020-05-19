use bigdecimal::BigDecimal;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{Duration, Utc};

use ethkey::prelude::*;

use std::convert::TryInto;

use std::future::Future;

use std::pin::Pin;

use uuid::Uuid;
use ya_payment_driver::account::AccountBalance;
use ya_payment_driver::gnt::GntDriver;
use ya_payment_driver::payment::{PaymentAmount, PaymentStatus};
use ya_payment_driver::AccountMode;
use ya_payment_driver::PaymentDriver;

use ya_persistence::executor::DbExecutor;

const KEYSTORE: &str = "/tmp/keystore.json";
const PASSWORD: &str = "";

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

fn get_account(keystore: &str, password: &str) -> Box<EthAccount> {
    EthAccount::load_or_generate(keystore, password).expect("should load or generate new eth key")
}

fn get_address(key: &Box<EthAccount>) -> String {
    hex::encode(key.address())
}

async fn show_balance(gnt_driver: &GntDriver, address: &str) {
    let balance_result = gnt_driver.get_account_balance(address).await;
    let balance: AccountBalance = balance_result.unwrap();
    println!("{:?}", balance);
}

async fn show_payment_status(gnt_driver: &GntDriver, invoice_id: &str) {
    match gnt_driver.get_payment_status(invoice_id).await.unwrap() {
        PaymentStatus::Ok(confirmation) => {
            println!("Payment status of: {} is Ok", invoice_id);
            let details = gnt_driver.verify_payment(&confirmation).await.unwrap();
            println!("{:?}", details);
        }
        _status => println!("Payment status of: {} is {:?}", invoice_id, _status),
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var(
        "RUST_LOG",
        "debug,tokio_core=info,tokio_reactor=info,hyper=info,web3::transports::http",
    );
    env_logger::init();
    dotenv::dotenv().expect("Failed to read .env file");
    let account = get_account(KEYSTORE, PASSWORD);
    let address = get_address(&account);
    println!("Address: {:?}", address);
    let sign_tx = get_sign_tx(account);

    let db = DbExecutor::new("file:/tmp/gnt_driver.db")?;
    ya_payment_driver::dao::init(&db).await?;

    let gnt_driver = GntDriver::new(db).await?;

    gnt_driver
        .init(AccountMode::SEND, address.as_str(), &sign_tx)
        .await
        .unwrap();

    show_balance(&gnt_driver, address.as_str()).await;

    let uuid = Uuid::new_v4().to_hyphenated().to_string();
    let invoice_id = uuid.as_str();
    let payment_amount = PaymentAmount {
        base_currency_amount: BigDecimal::from_str("69").unwrap(),
        gas_amount: None,
    };
    let due_date = Utc::now() + Duration::days(1i64);

    println!("Scheduling payment...");

    gnt_driver
        .schedule_payment(
            invoice_id,
            payment_amount,
            address.as_str(),
            address.as_str(),
            due_date,
            &sign_tx,
        )
        .await
        .map_or_else(|e| println!("{:?}", e), |_| println!("Done!"));

    show_balance(&gnt_driver, address.as_str()).await;
    show_payment_status(&gnt_driver, invoice_id).await;

    // let hardcoded: &str = "7e6fe400-92ba-4f93-960d-cc0720cf19e3";
    // show_payment_status(&gnt_driver, hardcoded).await;

    println!("Waiting for Ctr+C...");
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for event");

    Ok(())
}
