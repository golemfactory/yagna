//! Market API part of the Yagna API

mod provider;
mod requestor;

pub use provider::MarketProviderApi;
pub use requestor::MarketRequestorApi;

const MARKET_URL_ENV_VAR: &str = "YAGNA_MARKET_URL";
