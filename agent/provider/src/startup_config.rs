use humantime::Duration;
use std::error::Error;
use std::path::PathBuf;
use structopt::{clap, StructOpt};
use url::Url;

use ya_client::{
    activity::ActivityProviderApi, market::MarketProviderApi,
    payment::provider::ProviderApi as PaymentProviderApi, web::WebClient, web::WebInterface,
    Result,
};

/// Common configuration for all Provider commands.
#[derive(StructOpt)]
pub struct ProviderConfig {
    /// Descriptor file (JSON) for available ExeUnits
    #[structopt(
        long = "exe-unit-path",
        env = "EXE_UNIT_PATH",
        default_value = "/usr/lib/yagna/plugins/exeunits-descriptor.json"
    )]
    pub exe_unit_path: PathBuf,
    #[structopt(skip = "presets.json")]
    pub presets_file: PathBuf,
}

#[derive(StructOpt)]
pub struct RunConfig {
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    pub app_key: String,
    #[structopt(long = "node-name", env = "NODE_NAME", hide_env_values = true)]
    pub node_name: String,
    /// Market API URL
    #[structopt(long = "market-url", env = MarketProviderApi::API_URL_ENV_VAR)]
    market_url: Option<Url>,
    /// Activity API URL
    #[structopt(long = "activity-url", env = ActivityProviderApi::API_URL_ENV_VAR)]
    activity_url: Option<Url>,
    /// Payment API URL
    #[structopt(long = "payment-url", env = PaymentProviderApi::API_URL_ENV_VAR)]
    payment_url: Option<Url>,
    /// Estimated time of shutting down provider. Provider won't take any proposal,
    /// that expires after this time point.
    #[structopt(long)]
    pub shutdown: Duration,
    /// Credit address. Can be set same as default identity
    /// (will be removed in future release)
    #[structopt(long = "credit-address", env = "CREDIT_ADDRESS")]
    pub credit_address: String,
    /// Subnetwork identifier. You can set this value to filter nodes
    /// with other identifiers than selected. Useful for test purposes.
    #[structopt(long = "subnet", env = "SUBNET")]
    pub subnet: Option<String>,
    /// Offer presets, that will be sent to market.
    pub presets: Vec<String>,
}

#[derive(StructOpt)]
pub struct PresetNoInteractive {
    #[structopt(long)]
    pub preset_name: Option<String>,
    #[structopt(long)]
    pub exeunit: Option<String>,
    #[structopt(long)]
    pub pricing: Option<String>,
    #[structopt(long, parse(try_from_str = parse_key_val))]
    pub price: Vec<(String, f64)>,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum PresetsConfig {
    List,
    Create {
        #[structopt(long = "nointeractive")]
        nointeractive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    Remove {
        name: String,
    },
    Update {
        name: String,
        #[structopt(long = "nointeractive")]
        nointeractive: bool,
        #[structopt(flatten)]
        params: PresetNoInteractive,
    },
    ListMetrics,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub enum ExeUnitsConfig {
    List,
    // TODO: Install command - could download ExeUnit and add to descriptor file.
    // TODO: Update command - could update ExeUnit.
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub struct StartupConfig {
    #[structopt(flatten)]
    pub config: ProviderConfig,
    #[structopt(flatten)]
    pub commands: Commands,
}

#[derive(StructOpt)]
pub enum Commands {
    Run(RunConfig),
    Preset(PresetsConfig),
    ExeUnit(ExeUnitsConfig),
}

impl RunConfig {
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

/// Structopt key-value example:
/// https://github.com/TeXitoi/structopt/blob/master/examples/keyvalue.rs
fn parse_key_val<T, U>(s: &str) -> std::result::Result<(T, U), Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: Error + 'static,
    U: std::str::FromStr,
    U::Err: Error + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{}`", s))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}
