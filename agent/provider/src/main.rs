use actix::Actor;
use std::env;
use structopt::{clap, StructOpt};

use ya_provider::dir::clean_provider_dir;
use ya_provider::provider_agent::{ Initialize, ProviderAgent, Shutdown};
use ya_provider::signal::SignalMonitor;
use ya_provider::startup_config::{
    Commands, StartupConfig,
};
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
        Commands::Config(config_cmd) => config_cmd.run(config),
        Commands::Preset(presets_cmd) => presets_cmd.run(config),
        Commands::Profile(profile_cmd) => profile_cmd.run(config),
        Commands::ExeUnit(exe_unit_cmd) => exe_unit_cmd.run(config),
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
