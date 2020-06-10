#![allow(dead_code)]
#![allow(unused_macros)]

pub mod bcast;
pub mod mock_net;
pub mod mock_node;
pub mod mock_offer;

pub use mock_node::{MarketStore, MarketsNetwork};
pub use mock_offer::{example_demand, example_offer, mock_id};

macro_rules! assert_err_eq {
    ($expected:expr, $actual:expr $(,)*) => {
        assert_eq!($expected.to_string(), $actual.unwrap_err().to_string())
    };
}
