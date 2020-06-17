use std::env;
use structopt::{clap, StructOpt};

mod execution;
mod hardware;
mod market;
mod payments;
mod preset_cli;
mod provider_agent;
mod startup_config;
mod task_manager;
mod task_state;

use provider_agent::ProviderAgent;
use startup_config::{Commands, ExeUnitsConfig, PresetsConfig, StartupConfig};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli_args = StartupConfig::from_args();
    let mut config = cli_args.config;
    config.presets_file = config.data_dir.get_or_create()?.join(config.presets_file);

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
                no_interactive,
                params,
            } => {
                if no_interactive {
                    ProviderAgent::create_preset(config, params)
                } else {
                    ProviderAgent::create_preset_interactive(config)
                }
            }
            PresetsConfig::Remove { name } => ProviderAgent::remove_preset(config, name),
            PresetsConfig::Update {
                no_interactive,
                params,
                name,
            } => {
                if no_interactive {
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
