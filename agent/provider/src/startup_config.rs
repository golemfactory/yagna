use structopt::{clap, StructOpt};

use ya_client::cli::ApiOpts;

#[derive(StructOpt)]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub struct StartupConfig {
    #[structopt(flatten)]
    pub api: ApiOpts,

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
