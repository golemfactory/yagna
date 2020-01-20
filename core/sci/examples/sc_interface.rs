use ya_core_sci::SCInterface;

const GNT_TESTNET_CONTRACT: &str = "924442A66cFd812308791872C4B242440c108E19";
const GNTB_TESTNET_CONTRACT: &str = "123438d379BAbD07134d1d4d7dFa0BCbd56ca3F3";
const FAUCET_TESTNET_CONTRACT: &str = "77b6145E853dfA80E8755a4e824c4F510ac6692e";
const GETH_ADDRESS: &str = "http://188.165.227.180:55555";
const ETH_ADDRESS: &str = "09A14a40B3204A0B62c88a9ccD162CAc4fa0B4Ea";
const GNT_ADDRESS: &str = "d028d24f16a8893bd078259d413372ac01580769";
const GNTB_ADDRESS: &str = "d028d24f16a8893bd078259d413372ac01580769";

fn main() {
    let (_eloop, http) = web3::transports::Http::new(GETH_ADDRESS).unwrap();

    let mut sc_interface = SCInterface::new(http, ETH_ADDRESS);
    let sci = sc_interface
        .bind_gnt_contract(GNT_TESTNET_CONTRACT)
        .bind_gntb_contract(GNTB_TESTNET_CONTRACT)
        .bind_faucet_contract(FAUCET_TESTNET_CONTRACT);
    println!("Eth address: {:?}", sci.get_eth_address());
    println!("Eth balance: {:?}", sci.get_eth_balance(None));
    println!("Current block number: {:?}", sci.get_block_number());
    println!("Current Gas price: {:?}", sci.get_gas_price());
    println!("GNT balance: {:?}", sci.get_gnt_balance(GNT_ADDRESS));
    println!("GNTB balance: {:?}", sci.get_gntb_balance(GNTB_ADDRESS));
}
