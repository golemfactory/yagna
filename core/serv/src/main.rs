use actix_rt::SystemRunner;
use actix_web::{get, middleware, App, HttpServer, Responder};
use anyhow::{Context, Result};
use flexi_logger::Logger;
use futures::{lock::Mutex};
use std::{
    convert::{TryFrom, TryInto},
    fmt::Debug,
    path::PathBuf,
};
use structopt::{clap, StructOpt};

use ya_appkey::{error::Error, service::AppKeyService};
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, CommandOutput};

mod autocomplete;
use autocomplete::CompleteCommand;

#[derive(StructOpt, Debug)]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
struct CliArgs {
    /// Daemon data dir
    #[structopt(short, long = "datadir", set = clap::ArgSettings::Global)]
    data_dir: Option<PathBuf>,

    /// Daemon address
    #[structopt(short, long, default_value = "127.0.0.1")]
    address: String,

    /// Daemon HTTP port
    #[structopt(short, long, default_value = "7465")]
    http_port: u16,

    /// Service bus router port
    #[structopt(short = "l", default_value = "8245")]
    router_port: u16,

    /// Return results in JSON format
    #[structopt(long, set = clap::ArgSettings::Global)]
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

    pub fn get_http_address(&self) -> Result<(String, u16)> {
        Ok((self.address.clone(), self.http_port))
    }

    pub fn get_router_address(&self) -> Result<(String, u16)> {
        Ok((self.address.clone(), self.router_port))
    }

    pub fn run_command(self) -> Result<()> {
        let mut sys = actix_rt::System::new(clap::crate_name!());
        let ctx: CliCtx = (&self).try_into()?;

        if let CliCommand::Service(service) = self.command {
            Ok(ctx.output(service.run_command(sys, &ctx)?))
        } else {
            let command = self.command;
            let run = async move {
                ctx.output(command.run_command(&ctx).await?);
                Ok::<_, anyhow::Error>(())
            };
            Ok(sys.block_on(run)?)
        }
    }
}

impl TryFrom<&CliArgs> for CliCtx {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> Result<Self, Self::Error> {
        let data_dir = args.get_data_dir();
        log::info!("Using data dir: {:?} ", data_dir);

        Ok(CliCtx {
            http_address: args.get_http_address()?,
            router_address: args.get_router_address()?,
            data_dir,
            json_output: args.json,
            interactive: args.interactive,
        })
    }
}

#[derive(StructOpt, Debug)]
enum CliCommand {
    /// AppKey management
    AppKey(ya_appkey::cli::AppKeyCommand),

    /// Identity management
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Id(ya_identity::cli::IdentityCommand),

    #[structopt(name = "complete")]
    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    Complete(CompleteCommand),

    /// Core service usage
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Service(ServiceCommand),
}

impl CliCommand {
    pub async fn run_command(self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            CliCommand::AppKey(appkey) => appkey.run_command(ctx).await,
            CliCommand::Complete(complete) => complete.run_command(ctx),
            CliCommand::Id(id) => id.run_command(ctx).await,
            CliCommand::Service(_) => anyhow::bail!("service should be handled elsewhere"),
        }
    }
}

#[derive(StructOpt, Debug)]
enum ServiceCommand {
    /// Runs server in foreground
    Run,
    /// Spawns daemon
    Start,
    /// Stops daemon
    Stop,
    /// Checks if daemon is running
    Status,
}

impl ServiceCommand {
    pub fn run_command(&self, sys: SystemRunner, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            Self::Run => {
                let name = clap::crate_name!();
                log::info!("Starting {} service!", name);

                actix_rt::spawn(ya_sb_router::bind_router(ctx.router_address()?));

                ya_identity::service::activate()?;
                ya_appkey::service::bind(&APP_KEY_SERVICE);

                HttpServer::new(|| {
                    App::new()
                        .wrap(middleware::Logger::default())
                        .service(index)
                })
                .bind(ctx.http_address())
                .context(format!("Failed to bind {:?}", ctx.http_address()))?
                .start();

                sys.run()?;

                log::info!("{} service finished!", name);
                Ok(CommandOutput::object(format!(
                    "\n{} daemon successfully finished.",
                    name
                ))?)
            }
            _ => anyhow::bail!("command service {:?} is not implemented yet", self),
        }
    }
}

// TODO: move this to app-key crate
lazy_static::lazy_static! {
    pub static ref APP_KEY_SERVICE: AppKeyService = {
        let db_file_path = "core/appkey/appkey.sqlite3";
        let db_executor: DbExecutor<Error> = DbExecutor::new(db_file_path).unwrap();
        AppKeyService::new(Mutex::new(db_executor))
    };
}

#[get("/")]
fn index() -> impl Responder {
    format!("Hello {}!", clap::crate_description!())
}

fn main() -> Result<()> {
    let args = CliArgs::from_args();

    Logger::with_env_or_str("info,actix_server=info,actix_web=info")
        .start()
        .unwrap();

    args.run_command()
}
