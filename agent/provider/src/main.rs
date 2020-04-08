use structopt::{clap, StructOpt};

mod execution;
mod market;
mod payments;
mod provider_agent;
mod startup_config;

use provider_agent::ProviderAgent;
use startup_config::{ExeUnitsConfig, PresetsConfig, StartupConfig};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let config = StartupConfig::from_args();
    match config {
        StartupConfig::Run(args) => {
            env_logger::init();

            let app_name = clap::crate_name!();
            log::info!("Starting {}...", app_name);

            ProviderAgent::new(args).await?.wait_for_ctrl_c().await
        },
        StartupConfig::Presets(presets_cmd) => match presets_cmd {
            PresetsConfig::List => unimplemented!(),
        },
        StartupConfig::ExeUnit(exeunit_cmd) => match exeunit_cmd {
            ExeUnitsConfig::List{exe_unit_path} => ProviderAgent::list_exeunits(exe_unit_path),
        },
    }
}
