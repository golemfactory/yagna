use anyhow::Result;
use structopt::{clap, StructOpt};

mod service;
mod settings;

/// Manage environments
#[derive(StructOpt, Debug)]
enum Environments {
    Show,
    Enable { name: String },
    Disable { name: String },
}

#[derive(StructOpt, Debug)]
enum SettingsCommand {
    Set(settings::Settings),
    Env(Environments),
}

#[derive(StructOpt)]
enum Commands {
    Run,
    Settings(SettingsCommand),
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
    env_logger::init();

    let cli_args: StartupConfig = StartupConfig::from_args();

    match cli_args.commands {
        Commands::Run => service::run().await,
        Commands::Settings(command) => match command {
            SettingsCommand::Set(set) => settings::run(set).await,
            SettingsCommand::Env(env) => {
                log::info!("env: {:?}", env);
                Ok(0)
            }
        },
        Commands::Status => {
            // TODO
            Ok(0)
        }
    }
}

#[tokio::main]
async fn main() {
    std::process::exit(match my_main().await {
        Ok(code) => code,
        Err(e) => {
            log::error!("Error: {:?}", e);
            1
        }
    });
}
