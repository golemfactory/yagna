//! Async bindings for the Yagna API (REST)

#[macro_use]
pub mod web;

pub mod activity;
pub mod market;
pub mod payment;

pub mod error;
pub use error::Error;

pub type Result<T> = std::result::Result<T, Error>;

pub trait ApiClient {
    type market: web::WebInterface;
    type activity: web::WebInterface;
    type payment: web::WebInterface;
}

pub struct Api<T: ApiClient> {
    pub market: T::market,
    pub activity: T::activity,
    pub payment: T::payment,
}

pub type RequestorApi = Api<Requestor>;
pub type ProviderApi = Api<Provider>;

pub struct Requestor;
pub struct Provider;

impl ApiClient for Requestor {
    type market = market::MarketRequestorApi;
    type activity = activity::ActivityRequestorApi;
    type payment = payment::requestor::RequestorApi;
}

impl ApiClient for Provider {
    type market = market::MarketProviderApi;
    type activity = activity::ActivityProviderApi;
    type payment = payment::provider::ProviderApi;
}

#[cfg(feature = "cli")]
pub mod cli;
