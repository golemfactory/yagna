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
    type Market: web::WebInterface;
    type Activity: web::WebInterface;
    type Payment: web::WebInterface;
}

pub struct Api<T: ApiClient> {
    pub market: T::Market,
    pub activity: T::Activity,
    pub payment: T::Payment,
}

pub type RequestorApi = Api<Requestor>;
pub type ProviderApi = Api<Provider>;

pub struct Requestor;
pub struct Provider;

impl ApiClient for Requestor {
    type Market = market::MarketRequestorApi;
    type Activity = activity::ActivityRequestorApi;
    type Payment = payment::requestor::RequestorApi;
}

impl ApiClient for Provider {
    type Market = market::MarketProviderApi;
    type Activity = activity::ActivityProviderApi;
    type Payment = payment::provider::ProviderApi;
}

#[cfg(feature = "cli")]
pub mod cli;
