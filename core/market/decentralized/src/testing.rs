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
    let mut current_level = lvl;
    let frames = &bt.frames();
    while current_level < frames.len() {
        let frame = &frames[current_level];
        match get_backtrace_symbol(frame) {
            Some(name) => {
                // Handle Backtrace.actual_start_index set incorrectly
                if name.starts_with("backtrace::capture::Backtrace::new::") {
                    // We're at the last frame from backtrace lib - the creation of Backtrace.
                    current_level = current_level + lvl;
                } else if name.starts_with("backtrace::") {
                    // We haven't reached the last frame from backtrace lib yet.
                    current_level += 1;
                } else {
                    return Some(name);
                }
                log::warn!(
                    "Wrong start index from backtrace lib. Adjusting. current_level={}",
                    current_level
                );
            }
            _ => return None,
        };
    }
    None
}

pub fn generate_backtraced_name(level: Option<usize>) -> String {
    let bt = backtrace::Backtrace::new();
    // 0th element should be this function. We'd like to know the caller
    if let Some(name) = get_symbol_at_level(&bt, level.unwrap_or(1)) {
        return name;
    }
    log::debug!("No backtrace support. Generating default name from UUIDv4");
    uuid::Uuid::new_v4().to_string().to_string()
}
