use ethereum_types::{H256, U256};
use ethsign::{KeyFile, Protected};
use std::convert::TryInto;
use std::{thread, time};
use ya_core_sci::SCInterface;
const GNT_TESTNET_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";
const GNTB_TESTNET_CONTRACT: &str = "123438d379BAbD07134d1d4d7dFa0BCbd56ca3F3";
const FAUCET_TESTNET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";
const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const ETH_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
const GNT_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
const GNTB_ADDRESS: &str = "2f7681bfd7c4f0bf59ad1907d754f93b63492b4e";
const RINKEBY_CHAIN_ID: u64 = 4;

fn to_bytes(bytes_ref: &[u8]) -> [u8; 32] {
    bytes_ref.try_into().expect("slice with incorrect length")
}

fn get_keyfile() -> KeyFile {
    let file = std::fs::File::open("/home/daniel/work/datadir/keys/keystore.json").unwrap();
    let key: KeyFile = serde_json::from_reader(file).unwrap();
    key
}

fn get_private_key(key_file: &KeyFile) -> H256 {
    let password: Protected = "@Golem1234".into();
    let secret = key_file.to_secret_key(&password).unwrap();
    H256(to_bytes(secret.private()))
}

fn main() {
    let (_eloop, http) = web3::transports::Http::new(GETH_ADDRESS).unwrap();

    let nonce = 20;

    let private_key = get_private_key(&get_keyfile());

    let mut sc_interface = SCInterface::new(http, RINKEBY_CHAIN_ID);

    let sci = sc_interface
        .bind_gnt_contract(GNT_TESTNET_CONTRACT)
        .bind_gntb_contract(GNTB_TESTNET_CONTRACT)
        .bind_faucet_contract(FAUCET_TESTNET_CONTRACT);

    println!("Current Gas price: {:?}", sci.get_gas_price());
    println!("Eth balance: {:?}", sci.get_eth_balance(ETH_ADDRESS, None));
    println!("Current block number: {:?}", sci.get_block_number());
    println!("GNT balance: {:?}", sci.get_gnt_balance(GNT_ADDRESS));
    println!("GNTB balance: {:?}", sci.get_gntb_balance(GNTB_ADDRESS));

    println!(
        "Tx hash (Transfer GNTB to oneself): {:?}",
        sci.transfer_gntb(
            U256::from(1000000000),
            GNTB_ADDRESS,
            U256::from(nonce),
            private_key
        )
    );

    let nonce = nonce + 1;

    thread::sleep(time::Duration::from_secs(60));
    println!("Current block number: {:?}", sci.get_block_number());
    println!("Eth balance: {:?}", sci.get_eth_balance(ETH_ADDRESS, None));
    println!("GNTB balance: {:?}", sci.get_gntb_balance(GNTB_ADDRESS));

    // println!(
    //     "Tx hash (Request GNT from Faucet): {:?}",
    //     sci.request_gnt_from_faucet(U256::from(nonce), private_key)
    // );

    // let nonce = nonce + 1;

    // println!("Eth balance: {:?}", sci.get_eth_balance(ETH_ADDRESS, None));
    // println!("GNT balance: {:?}", sci.get_gnt_balance(GNT_ADDRESS));

    println!(
        "Tx hash (Transfer GNT to oneself): {:?}",
        sci.transfer_gnt(
            U256::from(1000000000),
            GNT_ADDRESS,
            U256::from(nonce),
            private_key
        )
    );

    // TODO remove underscore
    let _nonce = nonce + 1;

    thread::sleep(time::Duration::from_secs(60));
    println!("Current block number: {:?}", sci.get_block_number());
    println!("Eth balance: {:?}", sci.get_eth_balance(ETH_ADDRESS, None));
    println!("GNT balance: {:?}", sci.get_gnt_balance(GNT_ADDRESS));
}
