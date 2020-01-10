use actix_web::{get, middleware, App, HttpServer};
use anyhow::{Context, Result};
use flexi_logger::Logger;
use futures::lock::Mutex;
use std::{
    convert::{TryFrom, TryInto},
    fmt::Debug,
    path::PathBuf,
    sync::Arc,
};
use structopt::{clap, StructOpt};

use ya_appkey::error::Error;
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

    pub async fn run_command(self) -> Result<()> {
        let ctx: CliCtx = (&self).try_into()?;

        ctx.output(self.command.run_command(&ctx).await?);
        Ok::<_, anyhow::Error>(())
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
            CliCommand::Service(service) => service.run_command(ctx).await,
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
    async fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            Self::Run => {
                let name = clap::crate_name!();
                log::info!("Starting {} service!", name);

                ya_sb_router::bind_router(ctx.router_address()?).await
                    .context("binding service bus router")?;
                // FIXME: gsb is not binding services remotely; just random one
                ya_identity::service::activate();
                ya_appkey::service::bind_gsb(DB_EXECUTOR.clone());

                HttpServer::new(|| {
                    App::new()
                        .wrap(middleware::Logger::default())
                        .service(index)
                })
                .bind(ctx.http_address())
                .context(format!(
                    "Failed to bind http server on {:?}",
                    ctx.http_address
                ))?
                .run()
                .await?;

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

// TODO: move this to app-key crate (?)
lazy_static::lazy_static! {
    pub static ref DB_EXECUTOR: Arc<Mutex<DbExecutor<Error>>> = {
        let db_file_path = "core/appkey/appkey.sqlite3";
        let db_executor = DbExecutor::new(db_file_path).unwrap();
        Arc::new(Mutex::new(db_executor))
    };
}

#[get("/")]
async fn index() -> String {
    format!("Hello {}!", clap::crate_description!())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let args: CliArgs = CliArgs::from_args();

    Logger::with_env_or_str("info,actix_server=info,actix_web=info")
        .start()
        .unwrap();

    args.run_command().await
}
