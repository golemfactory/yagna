//! Market API part of the Yagna API

use ya_model::market::{MARKET_API, YAGNA_MARKET_URL_ENV_VAR};

mod provider;
mod requestor;

pub use provider::MarketProviderApi;
pub use requestor::MarketRequestorApi;
