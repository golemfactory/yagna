use std::env;
use structopt::{clap, StructOpt};

mod execution;
mod market;
mod payments;
mod preset_cli;
mod provider_agent;
mod startup_config;

use provider_agent::ProviderAgent;
use startup_config::{Commands, ExeUnitsConfig, PresetsConfig, StartupConfig};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli_args = StartupConfig::from_args();
    let config = cli_args.config;
    match cli_args.commands {
        Commands::Run(args) => {
            env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("info".into()));
            env_logger::init();

            let app_name = clap::crate_name!();
            log::info!("Starting {}...", app_name);

            ProviderAgent::new(args, config)
                .await?
                .wait_for_ctrl_c()
                .await
        }
        Commands::Preset(presets_cmd) => match presets_cmd {
            PresetsConfig::List => ProviderAgent::list_presets(config),
            PresetsConfig::Create {
                nointeractive,
                params,
            } => {
                if nointeractive {
                    ProviderAgent::create_preset(config, params)
                } else {
                    ProviderAgent::create_preset_interactive(config)
                }
            }
            PresetsConfig::Remove { name } => ProviderAgent::remove_preset(config, name),
            PresetsConfig::Update {
                nointeractive,
                params,
                name,
            } => {
                if nointeractive {
                    ProviderAgent::update_preset(config, name, params)
                } else {
                    ProviderAgent::update_preset_interactive(config, name)
                }
            }
            PresetsConfig::ListMetrics => ProviderAgent::list_metrics(config),
        },
        Commands::ExeUnit(exeunit_cmd) => match exeunit_cmd {
            ExeUnitsConfig::List => ProviderAgent::list_exeunits(config),
        },
    }
}
