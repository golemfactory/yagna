#[macro_use]
extern crate diesel;

mod config;
mod bus;
mod db;
mod identity;
mod market;
mod matcher;
mod negotiation;
mod protocol;
mod rest_api;
mod utils;

#[cfg(feature = "testing")]
pub mod testing;

pub use market::MarketService;
