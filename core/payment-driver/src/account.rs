use ethereum_types::U256;
#[allow(unused)]
pub enum Chain {
    Mainnet,
    Rinkeby,
}
#[allow(unused)]
pub enum Currency {
    Eth,
    GNT,
}
#[allow(unused)]
pub struct Balance {
    currency: Currency,
    amount: U256,
}

#[allow(unused)]
pub struct AccountBalance {
    base_currency: Balance,
    gas: Option<Balance>,
}
