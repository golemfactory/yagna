#![allow(dead_code)]
#![allow(unused_macros)]

pub mod bcast;
pub mod mock_net;
pub mod mock_node;

pub use mock_node::{wait_for_bcast, MarketStore, MarketsNetwork};
pub use ya_market_decentralized::testing::mock_offer::{
    generate_identity, sample_client_demand, sample_client_offer, sample_demand, sample_offer,
};

macro_rules! assert_err_eq {
    ($expected:expr, $actual:expr $(,)*) => {
        assert_eq!($expected.to_string(), $actual.unwrap_err().to_string())
    };
}
