use anyhow::Result;

use std::env;
use structopt::{clap, StructOpt};

mod appkey;
mod service;
mod settings;
mod settings_show;
mod setup;
mod status;
mod utils;

#[derive(StructOpt, Debug)]
enum SettingsCommand {
    /// Change settings
    Set(settings::Settings),
    /// Show current settings
    Show,
}

#[allow(clippy::large_enum_variant)]
#[derive(StructOpt)]
enum Commands {
    Setup(setup::RunConfig),

    /// Run the golem provider
    Run(setup::RunConfig),

    /// Manage settings
    ///
    /// This can be used regardless of whether golem is running or not.
    Settings(SettingsCommand),

    /// Show provider status
    ///
    /// Requires golem running.
    Status,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::crate_version_commit!())]
struct StartupConfig {
    #[structopt(flatten)]
    commands: Commands,
}

async fn my_main() -> Result</*exit code*/ i32> {
    dotenv::dotenv().ok();
    setup::init()?;

    if env::var_os(env_logger::DEFAULT_FILTER_ENV).is_none() {
        env::set_var(env_logger::DEFAULT_FILTER_ENV, "info");
    }
    env_logger::init();

    let cli_args: StartupConfig = StartupConfig::from_args();

    match cli_args.commands {
        Commands::Setup(mut run_config) => setup::setup(&mut run_config, true).await,
        Commands::Run(run_config) => service::run(run_config).await,
        Commands::Settings(command) => match command {
            SettingsCommand::Set(set) => settings::run(set).await,
            SettingsCommand::Show => settings_show::run().await,
        },
        Commands::Status => status::run().await,
    }
}

pub fn banner() {
    eprintln!(
        include_str!("banner.txt"),
        version = env!("CARGO_PKG_VERSION"),
        git_commit = option_env!("GITHUB_SHA").unwrap_or("-"),
        build = option_env!("GITHUB_RUN_NUMBER").unwrap_or("-")
    )
}

#[actix_rt::main]
async fn main() {
    std::process::exit(match my_main().await {
        Ok(code) => code,
        Err(e) => {
            log::error!("Error: {:?}", e);
            1
        }
    });
}
