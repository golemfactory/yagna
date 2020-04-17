use structopt::{clap, StructOpt};
use url::Url;

use ya_client::{
    activity::ActivityProviderApi, market::MarketProviderApi,
    payment::provider::ProviderApi as PaymentProviderApi, web::WebClient, web::WebInterface,
    Result,
};

#[derive(StructOpt)]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub struct StartupConfig {
    /// Yagna daemon application key
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    pub app_key: String,
    /// Market API URL
    #[structopt(long = "market-url", env = MarketProviderApi::API_URL_ENV_VAR)]
    market_url: Option<Url>,
    /// Activity API URL
    #[structopt(long = "activity-url", env = ActivityProviderApi::API_URL_ENV_VAR)]
    activity_url: Option<Url>,
    /// Payment API URL
    #[structopt(long = "payment-url", env = PaymentProviderApi::API_URL_ENV_VAR)]
    payment_url: Option<Url>,
    /// Descriptor file (JSON) for available ExeUnits
    #[structopt(
        long = "exe-unit-path",
        env = "EXE_UNIT_PATH",
        default_value = "/usr/lib/yagna/plugins/exeunits-descriptor.json"
    )]
    pub exe_unit_path: String,
    /// Credit address. Can be set same as default identity
    /// (will be removed in future release)
    #[structopt(long = "credit-address", env = "CREDIT_ADDRESS")]
    pub credit_address: String,
}

impl StartupConfig {
    pub fn market_client(&self) -> Result<MarketProviderApi> {
        self.api_client(&self.market_url)
    }

    pub fn activity_client(&self) -> Result<ActivityProviderApi> {
        self.api_client(&self.activity_url)
    }

    pub fn payment_client(&self) -> Result<PaymentProviderApi> {
        self.api_client(&self.payment_url)
    }

    pub fn api_client<T: WebInterface>(&self, url: &Option<Url>) -> Result<T> {
        let client = WebClient::with_token(&self.app_key)?;
        match url.as_ref() {
            Some(url) => Ok(client.interface_at(url.clone())),
            None => Ok(client.interface()?),
        }
    }
}
