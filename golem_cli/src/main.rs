#![recursion_limit = "512"]

use anyhow::Result;

use std::env;
use std::io::Write;
use structopt::{clap, StructOpt};

mod appkey;
mod command;
mod platform;
mod service;
mod settings;
mod settings_show;
mod setup;
mod status;
mod terminal;
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
    #[structopt(setting = clap::AppSettings::Hidden)]
    Setup(setup::RunConfig),

    /// Run the golem provider
    Run(setup::RunConfig),

    /// Stop the golem provider
    Stop,

    /// Manage settings
    Settings(SettingsCommand),

    /// Manage trusted keys
    Keystore(ProviderCommand),

    /// Show provider status
    Status,

    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    Complete(CompleteCommand),

    #[structopt(external_subcommand)]
    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    Other(Vec<String>),
}

#[derive(StructOpt)]
pub struct ProviderCommand {
    args: Vec<String>,
}

#[derive(StructOpt)]
/// Generates autocomplete script from given shell
pub struct CompleteCommand {
    /// Describes which shell to produce a completions file for
    #[structopt(
    parse(try_from_str),
    possible_values = &clap::Shell::variants(),
    case_insensitive = true
    )]
    shell: clap::Shell,
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
struct StartupConfig {
    #[structopt(flatten)]
    commands: Commands,
}

async fn my_main() -> Result</*exit code*/ i32> {
    dotenv::dotenv().ok();
    if env::var_os(env_logger::DEFAULT_FILTER_ENV).is_none() {
        env::set_var(env_logger::DEFAULT_FILTER_ENV, "info");
    }
    env_logger::init();

    let cli_args: StartupConfig = StartupConfig::from_args();

    match cli_args.commands {
        Commands::Setup(run_config) => setup::setup(&run_config, true).await,
        Commands::Run(run_config) => service::run(run_config).await,
        Commands::Stop => service::stop().await,
        Commands::Settings(command) => match command {
            SettingsCommand::Set(set) => settings::run(set).await,
            SettingsCommand::Show => settings_show::run().await,
        },
        Commands::Status => status::run().await,
        Commands::Keystore(mut command) => {
            command.args.insert(0, "keystore".to_string());
            Ok(command::YaCommand::new()?
                .ya_provider()?
                .forward(command.args)
                .await?)
        }
        Commands::Complete(complete) => {
            let binary_name = clap::crate_name!();
            println!(
                "# generating {} completions for {}",
                binary_name, complete.shell
            );
            StartupConfig::clap().gen_completions_to(
                binary_name,
                complete.shell,
                &mut std::io::stdout(),
            );
            Ok(0)
        }
        Commands::Other(args) => {
            let cmd = crate::command::YaCommand::new()?;
            match cmd.yagna()?.forward(args).await? {
                1 => {
                    let mut clap = Commands::clap();
                    let _ = clap.print_help();
                    let _ = std::io::stdout().write_all(b"\r\n");
                    std::process::exit(101);
                }
                code => Ok(code),
            }
        }
    }
}

pub fn banner() {
    terminal::fade_in(&format!(
        include_str!("banner.txt"),
        version = ya_compile_time_utils::semver_str!(),
        git_commit = ya_compile_time_utils::git_rev(),
        date = ya_compile_time_utils::build_date(),
        build = ya_compile_time_utils::build_number_str().unwrap_or("-"),
    ))
    .unwrap();
}

#[actix_rt::main]
async fn main() {
    std::process::exit(match my_main().await {
        Ok(code) => code,
        Err(e) => {
            log::error!("{:?}", e);
            1
        }
    });
}
