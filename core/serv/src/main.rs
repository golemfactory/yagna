use actix_web::{middleware, web, App, HttpServer, Responder};
use anyhow::{Context, Result};
use std::{
    any::TypeId,
    collections::HashMap,
    convert::{TryFrom, TryInto},
    env,
    fmt::Debug,
    path::PathBuf,
};
use structopt::{clap, StructOpt};
use url::Url;

#[cfg(all(feature = "market-forwarding", feature = "market-decentralized"))]
compile_error!("To use `market-decentralized` pls do `--no-default-features`.");
#[cfg(all(feature = "market-decentralized", not(feature = "market-forwarding")))]
use ya_market_decentralized::MarketService;
#[cfg(feature = "market-forwarding")]
use ya_market_forwarding::MarketService;
#[cfg(not(any(feature = "market-forwarding", feature = "market-decentralized")))]
compile_error!("Either feature \"market-forwarding\" or \"market-decentralized\" must be enabled.");

use ya_activity::service::Activity as ActivityService;
use ya_identity::service::Identity as IdentityService;
use ya_net::Net as NetService;
use ya_payment::PaymentService;
use ya_payment_driver::PaymentDriverService;
use ya_persistence::executor::DbExecutor;
use ya_sb_proto::{DEFAULT_GSB_URL, GSB_URL_ENV_VAR};
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_interfaces::Provider;
use ya_service_api_web::{
    middleware::{auth, Identity},
    rest_api_host_port, DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR,
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
    /// Service data dir
    #[structopt(
        short,
        long = "datadir",
        set = clap::ArgSettings::Global,
        env = "YAGNA_DATADIR",
        default_value,
        hide_env_values = true,
    )]
    data_dir: DataDir,

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

    pub fn log_level(&self) -> String {
        match self.command {
            CliCommand::Service(ServiceCommand::Run(..)) => self.log_level.clone(),
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
            gsb_url: Some(args.gsb_url.clone()),
            json_output: args.json,
            interactive: args.interactive,
        })
    }
}

#[derive(Clone)]
struct ServiceContext {
    dbs: HashMap<TypeId, DbExecutor>,
    default_db: DbExecutor,
}

impl<S: 'static> Provider<S, DbExecutor> for ServiceContext {
    fn component(&self) -> DbExecutor {
        match self.dbs.get(&TypeId::of::<S>()) {
            Some(db) => db.clone(),
            None => self.default_db.clone(),
        }
    }
}

impl ServiceContext {
    fn make_entry<S: 'static>(path: &PathBuf, name: &str) -> Result<(TypeId, DbExecutor)> {
        Ok((TypeId::of::<S>(), DbExecutor::from_data_dir(path, name)?))
    }

    fn from_data_dir(path: &PathBuf, name: &str) -> Result<Self> {
        let default_db = DbExecutor::from_data_dir(path, name)?;
        let dbs = [
            Self::make_entry::<MarketService>(path, "market")?,
            Self::make_entry::<ActivityService>(path, "activity")?,
            Self::make_entry::<PaymentService>(path, "payment")?,
        ]
        .iter()
        .cloned()
        .collect();

        Ok(ServiceContext { default_db, dbs })
    }
}

#[ya_service_api_derive::services(ServiceContext)]
enum Services {
    #[enable(gsb, cli(flatten))]
    Identity(IdentityService),
    #[enable(gsb)]
    Net(NetService),
    #[enable(gsb, rest)]
    Market(MarketService),
    #[enable(gsb, rest)]
    Activity(ActivityService),
    #[enable(gsb, rest, cli)]
    Payment(PaymentService),
    #[enable(gsb)]
    PaymentDriver(PaymentDriverService),
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
    Run(ServiceCommandOpts),
    /// Spawns daemon
    Start(ServiceCommandOpts),
    /// Stops daemon
    Stop,
    /// Checks if daemon is running
    Status,
}

#[derive(StructOpt, Debug)]
struct ServiceCommandOpts {
    /// Service address
    #[structopt(
        short,
        long,
        env = YAGNA_API_URL_ENV_VAR,
        default_value = DEFAULT_YAGNA_API_URL,
        hide_env_values = true,
    )]
    api_url: Url,
}

impl ServiceCommand {
    async fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            Self::Run(ServiceCommandOpts { api_url }) => {
                let name = clap::crate_name!();
                log::info!("Starting {} service!", name);

                ya_sb_router::bind_gsb_router(ctx.gsb_url.clone())
                    .await
                    .context("binding service bus router")?;

                let context = ServiceContext::from_data_dir(&ctx.data_dir, name)?;
                Services::gsb(&context).await?;

                let api_host_port = rest_api_host_port(api_url.clone());
                HttpServer::new(move || {
                    let app = App::new()
                        .wrap(middleware::Logger::default())
                        .wrap(auth::Auth::default())
                        .route("/me", web::get().to(me));
                    Services::rest(app, &context)
                })
                .bind(api_host_port.clone())
                .context(format!("Failed to bind http server on {:?}", api_host_port))?
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
