// TODO: This is only temporary as long there's only market structure.
//       Remove as soon as possible.
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

mod api;
mod db;
mod market;
mod matcher;
mod negotiation;

pub mod protocol;
pub use db::models::{Demand, Offer, SubscriptionId};
pub use market::MarketService;

pub use ya_client::model::market::MARKET_API_PATH;

#[macro_use]
extern crate diesel;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

// Exports only for service tests.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
pub use db::dao::{DemandDao, OfferDao};
