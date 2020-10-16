use actix::Actor;
use std::env;
use structopt::{clap, StructOpt};

mod cli;
mod events;
mod execution;
mod hardware;
mod market;
mod payments;
mod preset_cli;
mod provider_agent;
mod signal;
mod startup_config;
mod task_manager;
mod task_state;

use crate::hardware::Profiles;
use crate::provider_agent::{Initialize, Shutdown};
use crate::signal::SignalMonitor;
use provider_agent::ProviderAgent;
use startup_config::{
    Commands, ConfigConfig, ExeUnitsConfig, PresetsConfig, ProfileConfig, StartupConfig,
};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli_args = StartupConfig::from_args();
    let mut config = cli_args.config;
    let data_dir = config.data_dir.get_or_create()?;
    config.globals_file = data_dir.join(config.globals_file);
    config.presets_file = data_dir.join(config.presets_file);
    config.hardware_file = data_dir.join(config.hardware_file);

    match cli_args.commands {
        Commands::Run(args) => {
            env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("info".into()));
            env_logger::init();

            let app_name = clap::crate_name!();
            log::info!("Starting {}...", app_name);
            log::info!("Data directory: {}", data_dir.display());

            let agent = ProviderAgent::new(args, config).await?.start();
            agent.send(Initialize).await??;

            let (_, signal) = SignalMonitor::default().await;
            log::info!(
                "{} received, Shutting down {}...",
                signal,
                clap::crate_name!()
            );
            agent.send(Shutdown).await??;
            Ok(())
        }
        Commands::Config(config_cmd) => match config_cmd {
            ConfigConfig::Get { name } => cli::config_get(config, name),
            ConfigConfig::Set(node_config) => {
                let mut state = provider_agent::GlobalsState::load_or_create(&config.globals_file)?;
                state.update_and_save(node_config, &config.globals_file)?;
                Ok(())
            }
        },
        Commands::Preset(presets_cmd) => match presets_cmd {
            PresetsConfig::List => cli::list_presets(config),
            PresetsConfig::Active => cli::active_presets(config),
            PresetsConfig::Create {
                no_interactive,
                params,
            } => {
                if no_interactive {
                    cli::create_preset(config, params)
                } else {
                    cli::create_preset_interactive(config)
                }
            }
            PresetsConfig::Remove { name } => cli::remove_preset(config, name),
            PresetsConfig::Update {
                no_interactive,
                params,
                mut names,
            } => {
                if no_interactive {
                    cli::update_presets(&config, names, params)
                } else {
                    if names.all || names.names.len() != 1 {
                        anyhow::bail!("choose one name for interactive update");
                    }
                    cli::update_preset_interactive(config, names.names.drain(..).next().unwrap())
                }
            }
            PresetsConfig::Activate { name } => cli::activate_preset(config, name),
            PresetsConfig::Deactivate { name } => cli::deactivate_preset(config, name),
            PresetsConfig::ListMetrics => cli::list_metrics(config),
        },
        Commands::Profile(profile_cmd) => {
            let path = config.hardware_file.as_path();
            match profile_cmd {
                ProfileConfig::List => {
                    let profiles = Profiles::load_or_create(&config)?.list();
                    println!("{}", serde_json::to_string_pretty(&profiles)?);
                }
                ProfileConfig::Create { name, resources } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    if let Some(_) = profiles.get(&name) {
                        return Err(hardware::ProfileError::AlreadyExists(name).into());
                    }
                    profiles.add(name, resources)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Update { names, resources } => {
                    cli::update_profiles(config, names, resources)?;
                }
                ProfileConfig::Remove { name } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    profiles.remove(name)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Activate { name } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    profiles.set_active(name)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Active => {
                    let profiles = Profiles::load_or_create(&config)?;
                    println!("{}", serde_json::to_string_pretty(profiles.active())?);
                }
            }
            Ok(())
        }
        Commands::ExeUnit(exeunit_cmd) => match exeunit_cmd {
            ExeUnitsConfig::List => cli::list_exeunits(config),
        },
    }
}
