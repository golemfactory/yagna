mod db;
mod market;
mod matcher;
mod negotiation;
mod rest_api;

pub mod protocol;
pub mod testing;

pub use db::models::{Demand, Offer, ProposalId, SubscriptionId};
pub use market::MarketService;

pub use ya_client::model::market::MARKET_API_PATH;

#[macro_use]
extern crate diesel;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}
