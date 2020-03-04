use actix_rt;

use chrono::{Duration, Utc};

use ethereum_types::U256;

use ethsign::{KeyFile, Protected};

use ethkey::prelude::*;

use std::{thread, time};

use futures::{future, Future};
use std::pin::Pin;
use ya_payment_driver::account::{AccountBalance, Chain};
use ya_payment_driver::ethereum::EthereumClient;
use ya_payment_driver::gnt::GntDriver;
use ya_payment_driver::payment::{PaymentAmount, PaymentConfirmation};
use ya_payment_driver::PaymentDriver;

use ya_persistence::executor::DbExecutor;

const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const GNT_RINKEBY_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";

const ETH_FAUCET_ADDRESS: &str = "http://188.165.227.180:4000/donate";
const GNT_FAUCET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";

const KEYSTORE: &str = "/tmp/keystore.json";
const PASSWORD: &str = "";

const SLEEP_TIME: u64 = 60;

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

fn wait_for_confirmations() {
    let sleep_time = time::Duration::from_secs(SLEEP_TIME);
    println!("Waiting {:?} seconds for confirmations...", SLEEP_TIME);
    thread::sleep(sleep_time);
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

    let (_eloop, transport) = web3::transports::Http::new(GETH_ADDRESS)?;
    let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);

    let address: ethereum_types::Address = address.parse().unwrap();
    let gnt_contract_address: ethereum_types::Address = GNT_RINKEBY_CONTRACT.parse()?;
    let gnt_faucet_address: ethereum_types::Address = GNT_FAUCET_CONTRACT.parse()?;

    let db = DbExecutor::new("file:/tmp/gnt_driver.db")?;
    let mut gnt_driver = GntDriver::new(ethereum_client, gnt_contract_address, db)?;

    gnt_driver
        .init_funds(address, ETH_FAUCET_ADDRESS, gnt_faucet_address, &sign_tx)
        .await
        .unwrap();

    wait_for_confirmations();
    show_balance(&gnt_driver, address).await;

    let invoice_id = "invoice_1234";
    let payment_amount = PaymentAmount {
        base_currency_amount: U256::from(10000),
        gas_amount: None,
    };
    let due_date = Utc::now() + Duration::days(1i64);

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

    wait_for_confirmations();
    show_balance(&gnt_driver, address).await;

    let payment_status = gnt_driver.get_payment_status(invoice_id).await?;
    println!("Payment status: {:?}", payment_status);

    let tx_hash: Vec<u8> =
        hex::decode("9e3264d7a4a71934ee67e5abd520e1523f901b4a3f0316c905088e99d075737f").unwrap();
    let confirmation = PaymentConfirmation::from(&tx_hash);
    let details = gnt_driver.verify_payment(&confirmation).await?;
    println!("{:?}", details);

    Ok(())
}
