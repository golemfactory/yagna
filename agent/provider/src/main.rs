use actix::Actor;
use std::env;
use structopt::{clap, StructOpt};

use ya_provider::dir::clean_provider_dir;
use ya_provider::hardware::Profiles;
use ya_provider::provider_agent::{GlobalsState, Initialize, ProviderAgent, Shutdown};
use ya_provider::signal::SignalMonitor;
use ya_provider::startup_config::{
    Commands, ConfigConfig, ExeUnitsConfig, PresetsConfig, ProfileConfig, StartupConfig,
};
use ya_provider::{cli, hardware};
use ya_utils_process::lock::ProcLock;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli_args = StartupConfig::from_args();
    match &cli_args.commands {
        Commands::Run(_) => (), // logging is handled by ProviderAgent
        _ => {
            ya_file_logging::start_logger("info", None, &vec![], false)?;
            ()
        }
    }

    let mut config = cli_args.config;
    let data_dir = config.data_dir.get_or_create()?;

    config.globals_file = data_dir.join(config.globals_file);
    config.presets_file = data_dir.join(config.presets_file);
    config.hardware_file = data_dir.join(config.hardware_file);

    match cli_args.commands {
        Commands::Run(args) => {
            let app_name = clap::crate_name!();
            let _lock = ProcLock::new(&app_name, &data_dir)?.lock(std::process::id())?;
            let agent = ProviderAgent::new(args, config).await?.start();
            agent.send(Initialize).await??;

            let (_, signal) = SignalMonitor::default().await;
            log::info!("{} received, Shutting down {}...", signal, app_name);
            agent.send(Shutdown).await??;
            Ok(())
        }
        Commands::Config(config_cmd) => match config_cmd {
            ConfigConfig::Get { name } => cli::config_get(config, name),
            ConfigConfig::Set(node_config) => {
                let mut state = GlobalsState::load_or_create(&config.globals_file)?;
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
        Commands::Clean(clean_cmd) => {
            println!("Using data dir: {}", data_dir.display());

            let freed = clean_provider_dir(data_dir, clean_cmd.expr, true, clean_cmd.dry_run)?;
            let human_freed = bytesize::to_string(freed, false);

            if clean_cmd.dry_run {
                println!("Dry run: {} to be freed", human_freed)
            } else {
                println!("Freed {} of disk space", human_freed)
            }

            Ok(())
        }
    }
}
