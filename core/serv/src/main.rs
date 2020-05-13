use actix_web::{middleware, web, App, HttpServer, Responder};
use anyhow::{Context, Result};
use std::{
    convert::{TryFrom, TryInto},
    env,
    fmt::Debug,
    path::PathBuf,
};
use structopt::{clap, StructOpt};
use url::Url;

use ya_persistence::executor::DbExecutor;
use ya_sb_proto::{DEFAULT_GSB_URL, GSB_URL_ENV_VAR};
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_derive::services;
use ya_service_api_interfaces::Provider;
use ya_service_api_web::{
    middleware::{auth, Identity},
    DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR,
};

mod autocomplete;
use autocomplete::CompleteCommand;

mod data_dir;
use data_dir::DataDir;

#[derive(StructOpt, Debug)]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
struct CliArgs {
    /// Daemon data dir
    #[structopt(
        short,
        long = "datadir",
        set = clap::ArgSettings::Global,
        env = "YAGNA_DATADIR",
        default_value,
        hide_env_values = true,
    )]
    data_dir: DataDir,

    /// Daemon address
    #[structopt(
        short,
        long,
        env = YAGNA_API_URL_ENV_VAR,
        default_value = DEFAULT_YAGNA_API_URL,
        hide_env_values = true,
    )]
    api_url: Url,

    /// Service Bus (aka GSB) URL
    #[structopt(
        short,
        long,
        env = GSB_URL_ENV_VAR,
        default_value = DEFAULT_GSB_URL,
        hide_env_values = true
    )]
    gsb_url: Url,

    /// Return results in JSON format
    #[structopt(long, set = clap::ArgSettings::Global)]
    json: bool,

    /// Enter interactive mode
    #[structopt(short, long)]
    interactive: bool,

    /// Log verbosity level
    #[structopt(long, default_value = "info")]
    log_level: String,

    #[structopt(subcommand)]
    command: CliCommand,
}

impl CliArgs {
    pub fn get_data_dir(&self) -> Result<PathBuf> {
        self.data_dir.get_or_create()
    }

    pub fn get_http_address(&self) -> Result<(String, u16)> {
        let host = self
            .api_url
            .host()
            .ok_or_else(|| anyhow::anyhow!("invalid api url"))?
            .to_owned();
        let port = self
            .api_url
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("invalid api url, no port"))?;
        Ok((host.to_string(), port))
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
            data_dir,
            http_address: args.get_http_address()?,
            gsb_url: Some(args.gsb_url.clone()),
            json_output: args.json,
            interactive: args.interactive,
        })
    }
}

struct ServiceContext {
    db: DbExecutor,
    data_dir: PathBuf,
}

impl<Service> Provider<Service, DbExecutor> for ServiceContext {
    fn component(&self) -> DbExecutor {
        self.db.clone()
    }
}

impl<Service> Provider<Service, PathBuf> for ServiceContext {
    fn component(&self) -> PathBuf {
        self.data_dir.clone()
    }
}

#[services(ServiceContext)]
enum Services {
    #[enable(gsb, cli(flatten))]
    Identity(ya_identity::service::Identity),
    #[enable(gsb)]
    Net(ya_net::Net),
    #[enable(gsb, rest)]
    Activity(ya_activity::service::Activity),
    #[enable(gsb)]
    Market(ya_market::service::MarketService),
    #[enable(gsb, rest, cli)]
    Payment(ya_payment::PaymentService),
}

#[derive(StructOpt, Debug)]
enum CliCommand {
    #[structopt(flatten)]
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Commands(Services),

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
            CliCommand::Commands(command) => command.run_command(ctx).await,
            CliCommand::Complete(complete) => complete.run_command(ctx),
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

                ya_sb_router::bind_gsb_router(ctx.gsb_url.clone())
                    .await
                    .context("binding service bus router")?;

                let db = DbExecutor::from_data_dir(&ctx.data_dir, name)?;
                db.apply_migration(ya_persistence::migrations::run_with_output)?;
                let context = ServiceContext {
                    db: db.clone(),
                    data_dir: ctx.data_dir.clone(),
                };

                Services::gsb(&context).await?;

                HttpServer::new(move || {
                    let app = App::new()
                        .wrap(middleware::Logger::default())
                        .wrap(auth::Auth::default())
                        .route("/me", web::get().to(me));
                    Services::rest(app, &db)
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

async fn me(id: Identity) -> impl Responder {
    web::Json(id)
}

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let args: CliArgs = CliArgs::from_args();

    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or(args.log_level()));
    env_logger::init();

    std::env::set_var(GSB_URL_ENV_VAR, args.gsb_url.as_str()); // FIXME

    args.run_command().await
}
