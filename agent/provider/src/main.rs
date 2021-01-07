use actix::Actor;
use chrono::{DateTime, SecondsFormat, Utc};
use std::{env, path::Path};
use structopt::{clap, StructOpt};

mod cli;
mod dir;
mod events;
mod execution;
mod hardware;
mod market;
mod payments;
mod preset_cli;
mod provider_agent;
mod signal;
mod startup_config;
mod tasks;

use crate::dir::clean_provider_dir;
use crate::hardware::Profiles;
use crate::provider_agent::{Initialize, Shutdown};
use crate::signal::SignalMonitor;
use provider_agent::ProviderAgent;
use startup_config::{
    Commands, ConfigConfig, ExeUnitsConfig, PresetsConfig, ProfileConfig, StartupConfig,
};
use ya_utils_process::lock::ProcLock;

fn set_logging(log_level: &str, log_dir: &Path) -> anyhow::Result<()> {
    use flexi_logger::{
        style, AdaptiveFormat, Age, Cleanup, Criterion, DeferredNow, Duplicate, Logger, Naming,
        Record,
    };

    fn log_format(
        w: &mut dyn std::io::Write,
        now: &mut DeferredNow,
        record: &Record,
    ) -> Result<(), std::io::Error> {
        write!(
            w,
            "[{} {:5} {}] {}",
            DateTime::<Utc>::from(*now.now()).to_rfc3339_opts(SecondsFormat::Secs, true),
            record.level(),
            record.module_path().unwrap_or("<unnamed>"),
            record.args()
        )
    }

    fn log_format_color(
        w: &mut dyn std::io::Write,
        now: &mut DeferredNow,
        record: &Record,
    ) -> Result<(), std::io::Error> {
        let level = record.level();
        write!(
            w,
            "[{} {:5} {}] {}",
            DateTime::<Utc>::from(*now.now()).to_rfc3339_opts(SecondsFormat::Secs, true),
            style(level, level),
            record.module_path().unwrap_or("<unnamed>"),
            record.args()
        )
    }

    let mut logger = Logger::with_env_or_str(log_level).format(log_format);
    if log_dir.components().count() != 0 {
        logger = logger
            .log_to_file()
            .directory(log_dir)
            .rotate(
                Criterion::AgeOrSize(Age::Day, /*size in bytes*/ 1024 * 1024 * 1024),
                Naming::Timestamps,
                Cleanup::KeepLogAndCompressedFiles(1, 10),
            )
            .print_message()
            .duplicate_to_stderr(Duplicate::All);
    }
    logger = logger
        .adaptive_format_for_stderr(AdaptiveFormat::Custom(log_format, log_format_color))
        .set_palette("9;11;2;7;8".to_string());

    logger.start()?;
    Ok(())
}

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
            set_logging("info", &data_dir)?;

            let app_name = clap::crate_name!();
            log::info!("Starting {}...", app_name);
            log::info!("Data directory: {}", data_dir.display());

            log::info!("Performing disk cleanup...");
            let freed = clean_provider_dir(&data_dir, "30d", false, false)?;
            let human_freed = bytesize::to_string(freed, false);
            log::info!("Freed {} of disk space", human_freed);

            let _lock = ProcLock::new("ya-provider", &data_dir)?.lock(std::process::id())?;
            let agent = ProviderAgent::new(args, config).await?.start();
            agent.send(Initialize).await??;

            let (_, signal) = SignalMonitor::default().await;
            log::info!(
                "{} received, Shutting down {}...",
                signal,
                clap::crate_name!()
            );
            log::logger().flush();
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
