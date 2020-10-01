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

fn adjust_backtrace_level(frames: &[backtrace::BacktraceFrame]) -> Option<usize> {
    // On some systems backtrace lib doesn't properly set actual_start_index
    let mut idx = 0;
    for frame in frames.iter() {
        if let Some(name) = get_backtrace_symbol(frame) {
            // Note: On windows there is no "::<hash>" suffix
            if name.starts_with("ya_market_decentralized::testing::generate_backtraced_name") {
                return Some(idx);
            }
        }
        idx += 1;
    }
    None
}

fn get_symbol_at_level(bt: &backtrace::Backtrace, lvl: usize) -> Option<String> {
    let frames = &bt.frames();
    match adjust_backtrace_level(&frames) {
        Some(adjustment) => {
            let frame = &frames[lvl + adjustment];
            return get_backtrace_symbol(frame);
        }
        _ => {
            log::trace!("Cannot find adjustment for symbol. lvl={}", lvl);
        }
    };
    None
}

pub fn generate_backtraced_name(level: Option<usize>) -> String {
    let bt = backtrace::Backtrace::new();
    // 0th element should be this function. We'd like to know the caller
    if let Some(name) = get_symbol_at_level(&bt, level.unwrap_or(1)) {
        log::debug!("Generated name: {} level: {:?} BT: {:#?}", name, level, bt);
        return name;
    }
    let u4 = uuid::Uuid::new_v4().to_string().to_string();
    log::error!(
        "No backtrace support. Generating default name from UUIDv4. uuid4={}, bt={:#?}",
        u4,
        bt
    );
    u4
}
