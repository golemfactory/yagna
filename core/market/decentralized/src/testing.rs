//! This module is to be used only in tests.
#![allow(dead_code)]
#![allow(unused_macros)]

pub use super::config::*;
pub use super::db::dao::*;
pub use super::db::model::*;
pub use super::matcher::{error::*, *};
pub use super::negotiation::{error::*, *};
pub use super::protocol::*;

pub mod bcast;
pub mod dao;
pub mod events_helper;
pub mod mock_agreement;
pub mod mock_identity;
pub mod mock_net;
pub mod mock_node;
pub mod mock_offer;
pub mod proposal_util;

pub use mock_node::{wait_for_bcast, MarketServiceExt, MarketsNetwork};
pub use mock_offer::{client, sample_demand, sample_offer};

pub fn generate_backtraced_name() -> String {
    let bt = backtrace::Backtrace::new();
    // 0th element should be this function. We'd like to know the caller
    let frame = &bt.frames()[1];
    for symbol in frame.symbols().iter() {
        if let Some(name) = symbol.name() {
            return name.to_string().to_string();
        }
    }
    log::debug!("No backtrace support. Generating default name from UUIDv4");
    uuid::Uuid::new_v4().to_string().to_string()
}
