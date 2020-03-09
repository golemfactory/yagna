use actix_rt;

use chrono::{Duration, Utc};

use ethereum_types::U256;

use ethsign::{KeyFile, Protected};

use ethkey::prelude::*;

use futures3::future;
use std::future::Future;

use std::pin::Pin;

use uuid::Uuid;

use ya_payment_driver::account::{AccountBalance, Chain};
use ya_payment_driver::gnt::GntDriver;
use ya_payment_driver::payment::{PaymentAmount, PaymentStatus};
use ya_payment_driver::PaymentDriver;

use ya_persistence::executor::DbExecutor;

const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const GNT_RINKEBY_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";

const ETH_FAUCET_ADDRESS: &str = "http://188.165.227.180:4000/donate";
const GNT_FAUCET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";

const KEYSTORE: &str = "/tmp/keystore.json";
const PASSWORD: &str = "";

fn sign_tx(bytes: Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
    let secret = get_secret_key(KEYSTORE, PASSWORD);

    // Sign the message
    let signature = secret.sign(&bytes).unwrap();

    // Prepare signature
    let mut v = Vec::with_capacity(65);
    v.push(signature.v);
    v.extend_from_slice(&signature.r[..]);
    v.extend_from_slice(&signature.s[..]);

    Box::pin(future::ready(v))
}

fn load_or_generate_account(keystore: &str, password: &str) {
    let _ = EthAccount::load_or_generate(keystore, password)
        .expect("should load or generate new eth key");
}

fn get_key(keystore: &str) -> KeyFile {
    let file = std::fs::File::open(keystore).unwrap();
    let key: KeyFile = serde_json::from_reader(file).unwrap();
    key
}

fn get_secret_key(keystore: &str, password: &str) -> SecretKey {
    let key = get_key(keystore);
    let pwd: Protected = password.into();
    let secret = key.to_secret_key(&pwd).unwrap();
    secret
}

fn get_address(key: KeyFile) -> String {
    let address: Vec<u8> = key.address.unwrap().0;
    hex::encode(address)
}

async fn show_balance(gnt_driver: &GntDriver, address: ethereum_types::Address) {
    let balance_result = gnt_driver.get_account_balance(address).await;
    let balance: AccountBalance = balance_result.unwrap();
    println!("{:?}", balance);
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    load_or_generate_account(KEYSTORE, PASSWORD);
    let key = get_key(KEYSTORE);

    let address = get_address(key);
    println!("Address: {:?}", address);

    let address: ethereum_types::Address = address.parse().unwrap();
    let gnt_contract_address: ethereum_types::Address = GNT_RINKEBY_CONTRACT.parse()?;
    let gnt_faucet_address: ethereum_types::Address = GNT_FAUCET_CONTRACT.parse()?;

    let db = DbExecutor::new("file:/tmp/gnt_driver.db")?;
    ya_payment_driver::dao::init(&db).await?;

    let mut gnt_driver = GntDriver::new(
        Chain::Rinkeby,
        GETH_ADDRESS,
        gnt_contract_address,
        ETH_FAUCET_ADDRESS,
        gnt_faucet_address,
        db,
    )?;

    gnt_driver.init_funds(address, &sign_tx).await.unwrap();

    show_balance(&gnt_driver, address).await;

    let uuid = Uuid::new_v4().to_hyphenated().to_string();
    let invoice_id = uuid.as_str();
    let payment_amount = PaymentAmount {
        base_currency_amount: U256::from(10000),
        gas_amount: None,
    };
    let due_date = Utc::now() + Duration::days(1i64);

    println!("Scheduling payment...");

    gnt_driver
        .schedule_payment(
            invoice_id,
            payment_amount,
            address,
            address,
            due_date,
            &sign_tx,
        )
        .await
        .unwrap();

    println!("Gnt transferred!");

    show_balance(&gnt_driver, address).await;

    match gnt_driver.get_payment_status(invoice_id).await? {
        PaymentStatus::Ok(confirmation) => {
            let details = gnt_driver.verify_payment(&confirmation).await?;
            println!("{:?}", details);
        }
        _status => println!("{:?}", _status),
    }

    Ok(())
}
