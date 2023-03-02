use web3::types::{H160, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct YagnaRawTransaction {
    /// Nonce value
    pub nonce: U256,
    /// Recipient, None when creating contract
    pub to: Option<H160>,
    /// Transferred value
    pub value: U256,
    /// Gas price
    #[serde(rename = "gasPrice")]
    pub gas_price: U256,
    /// Gas amount
    pub gas: U256,
    /// Transaction data
    pub data: Vec<u8>,
}
