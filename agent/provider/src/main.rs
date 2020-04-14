use structopt::{clap, StructOpt};

mod execution;
mod market;
mod payments;
mod preset_cli;
mod provider_agent;
mod startup_config;

use provider_agent::ProviderAgent;
use startup_config::{Commands, ExeUnitsConfig, PresetsConfig, StartupConfig};
use std::path::PathBuf;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli_args = StartupConfig::from_args();
    let config = cli_args.config;
    match cli_args.commands {
        Commands::Run(args) => {
            env_logger::init();

            let app_name = clap::crate_name!();
            log::info!("Starting {}...", app_name);

            ProviderAgent::new(args, config)
                .await?
                .wait_for_ctrl_c()
                .await
        }
        Commands::Presets(presets_cmd) => match presets_cmd {
            PresetsConfig::List => {
                ProviderAgent::list_presets(config, PathBuf::from("presets.json"))
            }
            PresetsConfig::Create => {
                ProviderAgent::create_preset(config, PathBuf::from("presets.json"))
            }
            PresetsConfig::Remove { name } => {
                ProviderAgent::remove_preset(config, PathBuf::from("presets.json"), name)
            }
            PresetsConfig::Update { name } => {
                ProviderAgent::update_preset(config, PathBuf::from("presets.json"), name)
            }
        },
        Commands::ExeUnit(exeunit_cmd) => match exeunit_cmd {
            ExeUnitsConfig::List => ProviderAgent::list_exeunits(config),
        },
    }
}
