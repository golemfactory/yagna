mod db;
mod market;
mod matcher;
mod negotiation;
mod rest_api;

pub(crate) mod protocol;

#[cfg(feature = "testing")]
pub mod testing;

pub(crate) use db::models::{Demand, Offer, ProposalId, SubscriptionId};
pub use market::MarketService;

pub use ya_client::model::market::MARKET_API_PATH;

#[macro_use]
extern crate diesel;

pub(crate) mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}
