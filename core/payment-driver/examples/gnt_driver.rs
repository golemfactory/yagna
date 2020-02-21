use chrono::{Duration, Utc};
use ethereum_types::U256;
use ethsign::{KeyFile, Protected};

use futures::executor::block_on;

use ya_payment_driver::account::{AccountBalance, Chain};
use ya_payment_driver::ethereum::EthereumClient;
use ya_payment_driver::gnt::GntDriver;
use ya_payment_driver::payment::PaymentAmount;
use ya_payment_driver::PaymentDriver;

// use ya_persistence::executor::DbExecutor;

const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const GNT_RINKEBY_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";
// const FAUCET_TESTNET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";
const ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

fn sign_tx(bytes: Vec<u8>) -> Vec<u8> {
    let file = std::fs::File::open("/home/daniel/work/datadir/keys/keystore.json").unwrap();
    let key: KeyFile = serde_json::from_reader(file).unwrap();
    let password: Protected = "@Golem1234".into();
    let secret = key.to_secret_key(&password).unwrap();

    // Sign the message
    let signature = secret.sign(&bytes).unwrap();

    let mut v = Vec::with_capacity(65);
    v.push(signature.v);
    v.extend_from_slice(&signature.r[..]);
    v.extend_from_slice(&signature.s[..]);

    v
}

fn main() {
    let (_eloop, transport) = web3::transports::Http::new(GETH_ADDRESS).unwrap();
    let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);

    let address: ethereum_types::Address = ADDRESS.parse().unwrap();
    let gnt_rinkeby_address: ethereum_types::Address = GNT_RINKEBY_CONTRACT.parse().unwrap();
    // let faucet_rinkeby_address: ethereum_types::Address = FAUCET_TESTNET_CONTRACT.parse().unwrap();

    let mut driver: GntDriver =
        GntDriver::new(address, ethereum_client, gnt_rinkeby_address).unwrap();

    let balance_result = block_on(driver.get_account_balance());
    let balance: AccountBalance = balance_result.unwrap();
    println!("{:?}", balance);

    // driver
    //     .bind_faucet_contract(faucet_rinkeby_address)
    //     .map_or_else(
    //         |e| {
    //             println!("Failed to bind faucet contract: {:?}", e);
    //         },
    //         |_| {
    //             block_on(driver.request_gnt_from_faucet(sign_tx)).map_or_else(
    //                 |e| {
    //                     println!("{:?}", e);
    //                 },
    //                 |tx| {
    //                     println!("Requested: {:?}", tx);
    //                 },
    //             );
    //         },
    //     );
    let payment_amount = PaymentAmount {
        base_currency_amount: U256::from(10000),
        gas_amount: Some(U256::from(55000)),
    };
    let due_date = Utc::now() + Duration::days(1i64);
    let transfer = block_on(driver.schedule_payment(
        "invoice_1234",
        payment_amount,
        address,
        due_date,
        sign_tx,
    ));
    transfer.map_or_else(
        |e| println!("Unexpected error: {:?}", e),
        |_| {
            println!("Transferred");
        },
    )
}
