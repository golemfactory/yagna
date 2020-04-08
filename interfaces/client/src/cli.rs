use structopt::StructOpt;
use url::Url;

use crate::{
    activity::ACTIVITY_URL_ENV_VAR, market::MARKET_URL_ENV_VAR, payment::PAYMENT_URL_ENV_VAR,
    web::WebClient, Api, ApiClient,
};
use std::convert::TryFrom;

const YAGNA_APPKEY_ENV_VAR: &str = "YAGNA_APPKEY";

#[derive(StructOpt)]
pub struct ApiOpts {
    /// Yagna daemon application key
    #[structopt(long = "app-key", env = YAGNA_APPKEY_ENV_VAR, hide_env_values = true)]
    app_key: String,

    /// Market API URL
    #[structopt(long = "market-url", env = MARKET_URL_ENV_VAR, hide_env_values = true)]
    market_url: Option<Url>,

    /// Activity API URL
    #[structopt(long = "activity-url", env = ACTIVITY_URL_ENV_VAR, hide_env_values = true)]
    activity_url: Option<Url>,

    /// Payment API URL
    #[structopt(long = "payment-url", env = PAYMENT_URL_ENV_VAR, hide_env_values = true)]
    payment_url: Option<Url>,
}

impl<T: ApiClient> TryFrom<ApiOpts> for Api<T> {
    type Error = crate::Error;

    fn try_from(cli: ApiOpts) -> Result<Self, Self::Error> {
        let client = WebClient::with_token(&cli.app_key)?;

        Ok(Self {
            market: client.interface(cli.market_url)?,
            activity: client.interface(cli.activity_url)?,
            payment: client.interface(cli.payment_url)?,
        })
    }
}
