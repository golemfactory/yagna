//! Market API part of the Yagna API
use std::{env, rc::Rc};
use url::Url;

mod provider;
mod requestor;

pub use provider::MarketProviderApi;
pub use requestor::MarketRequestorApi;

const MARKET_URL_ENV_VAR: &str = "YAGNA_MARKET_URL";
/// Centralized (Mk1 aka TestBed) Market API instance.
// TODO: remove it after implementing P2P Market
const DEFAULT_MARKET_URL: &str = "http://34.244.4.185:8080/market-api/v1/";

fn default_url() -> crate::Result<Rc<Url>> {
    Ok(env::var(MARKET_URL_ENV_VAR)
        .unwrap_or(DEFAULT_MARKET_URL.into())
        .parse::<Url>()?
        .into())
}
