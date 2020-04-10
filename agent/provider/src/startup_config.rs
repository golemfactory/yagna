use std::path::PathBuf;
use structopt::{clap, StructOpt};
use url::Url;

use ya_client::{
    activity::ActivityProviderApi, market::MarketProviderApi,
    payment::provider::ProviderApi as PaymentProviderApi, web::WebClient, web::WebInterface,
    Result,
};

#[derive(StructOpt)]
pub struct RunConfig {
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    pub auth: String,
    #[structopt(long = "node-name", env = "NODE_NAME", hide_env_values = true)]
    pub node_name: String,
    /// Market API URL
    #[structopt(long = "market-url", env = MarketProviderApi::API_URL_ENV_VAR)]
    market_url: Url,
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
    pub exe_unit_path: PathBuf,
    /// Credit address. Can be set same as default identity
    /// (will be removed in future release)
    #[structopt(long = "credit-address", env = "CREDIT_ADDRESS")]
    pub credit_address: String,
    /// Offer presets, that will be sent to market.
    pub presets: Vec<String>,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum PresetsConfig {
    List,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum ExeUnitsConfig {
    List {
        /// Descriptor file (JSON) for available ExeUnits
        #[structopt(
            long = "exe-unit-path",
            env = "EXE_UNIT_PATH",
            default_value = "/usr/lib/yagna/plugins/exeunits-descriptor.json"
        )]
        exe_unit_path: PathBuf,
    }, // TODO: Install command - could download ExeUnit and add to descriptor file.
       // TODO: Update command - could update ExeUnit.
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum StartupConfig {
    Run(RunConfig),
    Presets(PresetsConfig),
    ExeUnit(ExeUnitsConfig),
}

impl RunConfig {
    pub fn market_client(&self) -> Result<MarketProviderApi> {
        Ok(WebClient::with_token(&self.auth)?.interface_at(self.market_url.clone()))
    }

    pub fn activity_client(&self) -> Result<ActivityProviderApi> {
        let client = WebClient::with_token(&self.auth)?;
        if let Some(url) = &self.activity_url {
            Ok(client.interface_at(url.clone()))
        } else {
            Ok(client.interface()?)
        }
    }

    pub fn payment_client(&self) -> Result<PaymentProviderApi> {
        let client = WebClient::with_token(&self.auth)?;
        if let Some(url) = &self.payment_url {
            Ok(client.interface_at(url.clone()))
        } else {
            Ok(client.interface()?)
        }
    }
}
