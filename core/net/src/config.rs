use std::time::Duration;
use structopt::StructOpt;
use strum::VariantNames;
use strum::{EnumString, EnumVariantNames, IntoStaticStr};
use url::Url;

#[derive(StructOpt, EnumString, EnumVariantNames, IntoStaticStr, Copy, Clone)]
#[strum(serialize_all = "lowercase")]
pub enum NetType {
    Central,
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
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Config::from_iter_safe(&[""])
    }
}

/// TODO: Remove compilation flag.
///  This conditional compilation is hack to make Goth integration tests work.
///  Current solution in Goth is to build separate binary with compilation flag.
///  This is only temporary for transition period, to make this PR as small as possible.
#[cfg(feature = "central-net")]
impl Default for NetType {
    fn default() -> Self {
        std::env::set_var("YA_NET_TYPE", "central");
        NetType::Central
    }
}

#[cfg(not(feature = "central-net"))]
impl Default for NetType {
    fn default() -> Self {
        std::env::set_var("YA_NET_TYPE", "hybrid");
        NetType::Hybrid
    }
}
