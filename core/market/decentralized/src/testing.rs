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

fn get_backtrace_symbol(fm: &backtrace::BacktraceFrame) -> Option<String> {
    for symbol in fm.symbols().iter() {
        if let Some(name) = symbol.name() {
            return Some(name.to_string());
        }
    }
    None
}

fn get_symbol_at_level(bt: &backtrace::Backtrace, lvl: usize) -> Option<String> {
    let frame = &bt.frames()[lvl];
    get_backtrace_symbol(frame)
}

pub fn generate_backtraced_name(level_o: Option<usize>) -> String {
    let bt = backtrace::Backtrace::new();
    // 0th element should be this function. We'd like to know the caller
    let level = level_o.unwrap_or(1);
    if let Some(name) = get_symbol_at_level(&bt, level) {
        // Special case for Mac
        if name.starts_with("backtrace::capture::Backtrace::new::") {
            let adjusted_level = level + level;
            log::warn!(
                "Wrong start index from backtrace lib. Adjusting. adjusted_level={}",
                adjusted_level
            );
            if let Some(adjusted_name) = get_symbol_at_level(&bt, adjusted_level) {
                return adjusted_name;
            }
        } else {
            return name;
        }
    }
    log::debug!("No backtrace support. Generating default name from UUIDv4");
    uuid::Uuid::new_v4().to_string().to_string()
}
