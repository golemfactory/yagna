use structopt::{clap, StructOpt};

use ya_client::cli::ApiOpts;

#[derive(StructOpt)]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub struct StartupConfig {
    #[structopt(flatten)]
    pub api: ApiOpts,

    /// Descriptor file (JSON) for available ExeUnits
    #[structopt(
        long = "exe-unit-path",
        env = "EXE_UNIT_PATH",
        hide_env_values = true,
        default_value = "/usr/lib/yagna/plugins/exeunits-descriptor.json"
    )]
    pub exe_unit_path: String,
    /// Credit address. Can be set same as default identity.
    /// It will be removed in future release -- agreement will specify it.
    #[structopt(
        long = "credit-address",
        env = "CREDIT_ADDRESS",
        hide_env_values = true
    )]
    pub credit_address: String,
}
