/*
    Private mod to encapsulate all erc20 logic, revealed from the `wallet`.
*/

pub mod ethereum;
pub mod faucet;
pub mod utils;
pub mod wallet;

mod config;
pub mod eth_utils;
mod gasless_transfer;
pub mod transaction;
