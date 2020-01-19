use actix_web::{get, middleware, App, HttpServer};
use anyhow::{Context, Result};
use std::{
    convert::{TryFrom, TryInto},
    env,
    fmt::Debug,
    path::PathBuf,
};
use structopt::{clap, StructOpt};

use ya_core_model::identity;
use ya_persistence::executor::DbExecutor;
use ya_service_api::{
    constants::{CENTRAL_NET_HOST, YAGNA_BUS_PORT, YAGNA_HOST, YAGNA_HTTP_PORT},
    CliCtx, CommandOutput,
};
use ya_service_api_web::middleware::auth;
use ya_service_bus::{typed as bus, RpcEndpoint};

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
    #[structopt(short, long, default_value = &*YAGNA_HOST, env = "YAGNA_HOST")]
    address: String,

    /// Daemon HTTP port
    #[structopt(short = "p", long, default_value = &*YAGNA_HTTP_PORT, env = "YAGNA_HTTP_PORT")]
    http_port: u16,

    /// Service bus router port
    #[structopt(long, default_value = &*YAGNA_BUS_PORT, env = "YAGNA_BUS_PORT")]
    #[structopt(set = clap::ArgSettings::Global)]
    router_port: u16,

    /// Return results in JSON format
    #[structopt(long, set = clap::ArgSettings::Global)]
    json: bool,

    /// Enter interactive mode
    #[structopt(short, long)]
    interactive: bool,

    /// Log verbosity level
    #[structopt(long, default_value = "debug")]
    log_level: String,

    #[structopt(subcommand)]
    command: CliCommand,
}

impl CliArgs {
    pub fn get_data_dir(&self) -> Result<PathBuf> {
        Ok(match &self.data_dir {
            Some(data_dir) => data_dir.to_owned(),
            None => ya_service_api::default_data_dir()?,
        })
    }

    pub fn get_http_address(&self) -> Result<(String, u16)> {
        Ok((self.address.clone(), self.http_port))
    }

    pub fn get_router_address(&self) -> Result<(String, u16)> {
        Ok((self.address.clone(), self.router_port))
    }

    pub fn log_level(&self) -> String {
        match self.command {
            CliCommand::Service(ServiceCommand::Run) => self.log_level.clone(),
            _ => "error".to_string(),
        }
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
        let data_dir = args.get_data_dir()?;
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
    AppKey(ya_identity::cli::AppKeyCommand),

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

                ya_sb_router::bind_router(ctx.router_address()?)
                    .await
                    .context("binding service bus router")?;

                let db = DbExecutor::from_data_dir(&ctx.data_dir)?;

                db.apply_migration(ya_persistence::migrations::run_with_output)?;
                ya_identity::service::activate(&db).await?;
                ya_activity::provider::service::bind_gsb(&db);

                let default_id = bus::private_service(identity::IDENTITY_SERVICE_ID)
                    .send(identity::Get::ByDefault)
                    .await
                    .map_err(anyhow::Error::msg)??
                    .ok_or(anyhow::Error::msg("no default identity"))?
                    .node_id
                    .to_string();
                log::info!("using default identity as network id: {:?}", default_id);
                ya_net::bind_remote(&*CENTRAL_NET_HOST, &default_id)
                    .await
                    .context(format!(
                        "Error binding network service at {} for {}",
                        *CENTRAL_NET_HOST, default_id
                    ))?;

                HttpServer::new(move || {
                    App::new()
                        .wrap(middleware::Logger::default())
                        .wrap(auth::Auth::default())
                        .service(index)
                        .service(ya_activity::provider::web_scope(&db))
                        .service(ya_activity::requestor::control::web_scope(&db))
                        .service(ya_activity::requestor::state::web_scope(&db))
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

#[get("/")]
async fn index() -> String {
    format!("Hello {}!", clap::crate_description!())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let args: CliArgs = CliArgs::from_args();

    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or(args.log_level()));
    env_logger::init();

    args.run_command().await
}
