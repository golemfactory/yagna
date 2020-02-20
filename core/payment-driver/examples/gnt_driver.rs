use futures::executor::block_on;

use ya_payment_driver::account::{AccountBalance, Chain};
use ya_payment_driver::ethereum::EthereumClient;
use ya_payment_driver::gnt::GntDriver;
use ya_payment_driver::PaymentDriver;

const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const GNT_RINKEBY_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";
const FAUCET_TESTNET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";
const ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";

// TODO
fn sign_tx(tx: Vec<u8>) -> Vec<u8> {
    tx
}

fn main() {
    let (_eloop, transport) = web3::transports::Http::new(GETH_ADDRESS).unwrap();
    let ethereum_client = EthereumClient::new(transport, Chain::Rinkeby);

    let address: ethereum_types::Address = ADDRESS.parse().unwrap();
    let gnt_rinkeby_address: ethereum_types::Address = GNT_RINKEBY_CONTRACT.parse().unwrap();
    let faucet_rinkeby_address: ethereum_types::Address = FAUCET_TESTNET_CONTRACT.parse().unwrap();

    let mut driver: GntDriver =
        GntDriver::new(address, ethereum_client, gnt_rinkeby_address).unwrap();

    let balance_result = block_on(driver.get_account_balance());
    let balance: AccountBalance = balance_result.unwrap();

    driver
        .bind_faucet_contract(faucet_rinkeby_address)
        .map_or_else(
            |e| {
                println!("Failed to bind faucet contract: {:?}", e);
            },
            |_| {
                block_on(driver.request_gnt_from_faucet(sign_tx)).map_or_else(
                    |e| {
                        println!("{:?}", e);
                    },
                    |_| {
                        println!("Requested");
                    },
                );
            },
        );

    println!("{:?}", balance);
}
