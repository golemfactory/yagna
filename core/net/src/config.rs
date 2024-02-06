use std::time::Duration;
use structopt::StructOpt;
use strum::VariantNames;
use strum::{EnumString, EnumVariantNames, IntoStaticStr};
use url::Url;

// TODO: Remove compilation flag.
#[derive(
    StructOpt,
    EnumString,
    EnumVariantNames,
    IntoStaticStr,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Debug,
    Default,
)]
#[strum(serialize_all = "kebab-case")]
pub enum NetType {
    /// TODO: Remove compilation flag.
    ///  This conditional compilation is hack to make Goth integration tests work.
    ///  Current solution in Goth is to build separate binary with compilation flag.
    ///  This is only temporary for transition period, to make this PR as small as possible.
    #[cfg_attr(feature = "central-net", default)]
    Central,
    #[cfg_attr(not(feature = "central-net"), default)]
    Hybrid,
}

#[derive(StructOpt, Clone)]
#[structopt(rename_all = "kebab-case")]
pub struct Config {
    #[structopt(env = "YA_NET_TYPE", possible_values = NetType::VARIANTS, default_value = NetType::default().into())]
    pub net_type: NetType,
    #[structopt(env = "YA_NET_RELAY_HOST")]
    pub host: Option<String>,
    #[structopt(env = "YA_NET_BIND_URL", default_value = "udp://0.0.0.0:11500")]
    pub bind_url: Url,
    #[structopt(env = "YA_NET_BROADCAST_SIZE", default_value = "10")]
    pub broadcast_size: u32,
    #[structopt(env = "YA_NET_SESSION_EXPIRATION", parse(try_from_str = humantime::parse_duration), default_value = "15s")]
    pub session_expiration: Duration,
    #[structopt(env = "YA_NET_SESSION_REQUEST_TIMEOUT", parse(try_from_str = humantime::parse_duration), default_value = "3s")]
    pub session_request_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Config::from_iter_safe(&[""])
    }
}
