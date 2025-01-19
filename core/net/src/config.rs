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
    #[cfg_attr(feature = "central-net", default)]
    Central,
    Hybrid,
    #[cfg_attr(not(feature = "central-net"), default)]
    IROH,
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
    #[structopt(env = "YA_NET_BROADCAST_SIZE", default_value = "5")]
    pub broadcast_size: u32,
    #[structopt(env = "YA_NET_PUB_BROADCAST_SIZE", default_value = "30")]
    pub pub_broadcast_size: u32,
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
