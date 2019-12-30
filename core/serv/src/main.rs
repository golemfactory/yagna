use actix_web::{get, middleware, App, HttpServer, Responder};
use anyhow::{Context, Result};
use flexi_logger::Logger;
use std::{
    convert::{TryFrom, TryInto},
    fmt::Debug,
    path::PathBuf,
};
use structopt::{clap, StructOpt};

use ya_service_api::{CliCtx, Command, CommandOutput};

mod autocomplete;
pub use autocomplete::CompleteCommand;

#[derive(StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
struct CliArgs {
    /// Daemon data dir
    #[structopt(short, long = "datadir")]
    #[structopt(set = clap::ArgSettings::Global)]
    data_dir: Option<PathBuf>,

    /// Daemon address
    #[structopt(short, long)]
    #[structopt(default_value = "127.0.0.1")]
    address: String,

    /// Daemon port
    #[structopt(short, long)]
    #[structopt(default_value = "7465")]
    port: u16,

    /// Return results in JSON format
    #[structopt(long)]
    #[structopt(set = clap::ArgSettings::Global)]
    json: bool,

    /// Enter interactive mode
    #[structopt(short, long)]
    interactive: bool,

    #[structopt(subcommand)]
    command: CliCommand,
}

impl CliArgs {
    #[allow(dead_code)]
    pub fn get_data_dir(&self) -> PathBuf {
        match &self.data_dir {
            Some(data_dir) => data_dir.to_owned(),
            None => appdirs::user_data_dir(Some("yagna"), Some("golem"), false)
                .unwrap()
                .join("default"),
        }
    }

    pub fn get_address(&self) -> Result<(String, u16)> {
        Ok((self.address.clone(), self.port))
    }

    pub fn run_command(&self) -> Result<()> {
        let ctx: CliCtx = self.try_into()?;
        Ok(ctx.output(self.command.run_command(&ctx)?))
    }
}

impl TryFrom<&CliArgs> for CliCtx {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> Result<Self, Self::Error> {
        let data_dir = args.get_data_dir();
        log::info!("Using data dir: {:?} ", data_dir);
        let address = args.get_address()?;
        let json_output = args.json;
        //        let net = value.net.clone();
        //        let accept_any_prompt = args.accept_any_prompt;
        let interactive = args.interactive;
        //        let sys = actix::System::new("golemcli");

        Ok(CliCtx {
            address,
            data_dir,
            json_output,
            //            accept_any_prompt,
            //            net,
            interactive,
            //            sys,
        })
    }
}

#[derive(StructOpt, Debug)]
enum CliCommand {
    /// Core service usage
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Service(ServiceCommand),

    /// Identity management
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Id(ya_identity::IdentityCommand),

    #[structopt(name = "complete")]
    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    Complete(CompleteCommand),
}

impl Command for CliCommand {
    fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            CliCommand::Service(service) => service.run_command(ctx),
            CliCommand::Id(id) => id.run_command(ctx),
            CliCommand::Complete(complete) => complete.run_command(ctx),
        }
    }
}

#[derive(StructOpt, Debug)]
enum ServiceCommand {
    /// Runs server in foreground
    Run,
    /// Spawns daemon
    Start,
    /// Stops daemonm
    Stop,
    /// Checks if daemon is running
    Status,
}

impl Command for ServiceCommand {
    fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            Self::Run => {
                println!("Running {} service!", structopt::clap::crate_name!());
                HttpServer::new(|| {
                    App::new()
                        .wrap(middleware::Logger::default())
                        .service(index)
                })
                .bind(ctx.address())
                .context(format!("Failed to bind {:?}", ctx.address()))?
                .run()?;
                Ok(CommandOutput::NoOutput)
            }
            _ => anyhow::bail!("command service {:?} is not implemented yet", self),
        }
    }
}

#[get("/")]
fn index() -> impl Responder {
    format!("Hello {}!", clap::crate_description!())
}

fn main() -> Result<()> {
    let args = CliArgs::from_args();

    Logger::with_env_or_str("actix_server=info,actix_web=info")
        .start()
        .unwrap();

    args.run_command()
}
