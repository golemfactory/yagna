use structopt::StructOpt;
use strum::VariantNames;
use strum::{EnumString, EnumVariantNames, IntoStaticStr};

#[derive(StructOpt, EnumString, EnumVariantNames, IntoStaticStr, Clone)]
#[strum(serialize_all = "lowercase")]
pub enum NetType {
    Central,
    Hybrid,
}

#[derive(StructOpt, Clone)]
#[structopt(rename_all = "kebab-case")]
pub struct Config {
    #[structopt(env = "YA_NET_TYPE", possible_values = NetType::VARIANTS, default_value = NetType::Central.into())]
    pub net_type: NetType,
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Ok(Config::from_iter_safe(&[""])?)
    }
}

impl Default for NetType {
    fn default() -> Self {
        NetType::Central
    }
}
