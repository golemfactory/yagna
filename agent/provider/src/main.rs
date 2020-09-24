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
use startup_config::{Commands, ExeUnitsConfig, PresetsConfig, ProfileConfig, StartupConfig};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli_args = StartupConfig::from_args();
    let mut config = cli_args.config;
    let data_dir = config.data_dir.get_or_create()?;
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
                name,
            } => {
                if no_interactive {
                    cli::update_preset(config, name, params)
                } else {
                    cli::update_preset_interactive(config, name)
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
                ProfileConfig::Update { name, resources } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    match profiles.get_mut(&name) {
                        Some(profile) => *profile = resources,
                        _ => return Err(hardware::ProfileError::Unknown(name).into()),
                    }
                    profiles.save(path)?;
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
