#[macro_use]
extern crate diesel;

mod cli;
mod config;
mod db;
mod identity;
mod market;
mod matcher;
mod negotiation;
mod protocol;
mod rest_api;
mod utils;

pub mod testing;

pub use market::MarketService;
