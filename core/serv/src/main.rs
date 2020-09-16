use actix_web::{middleware, web, App, HttpServer, Responder};
use anyhow::{Context, Result};
use futures::lock::Mutex;
use std::{
    any::TypeId,
    collections::HashMap,
    convert::{TryFrom, TryInto},
    env,
    fmt::Debug,
    path::{Path, PathBuf},
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
use ya_payment::{accounts as payment_accounts, PaymentService};

use ya_metrics_utils::Statistics;
use ya_persistence::executor::DbExecutor;
use ya_sb_proto::{DEFAULT_GSB_URL, GSB_URL_ENV_VAR};
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_interfaces::Provider;
use ya_service_api_web::{
    middleware::{auth, Identity},
    rest_api_host_port, DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR,
};
use ya_utils_path::data_dir::DataDir;

mod autocomplete;
use autocomplete::CompleteCommand;
use std::sync::Arc;

lazy_static::lazy_static! {
    static ref DEFAULT_DATA_DIR: String = DataDir::new(clap::crate_name!()).to_string();
}

#[derive(StructOpt, Debug)]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::crate_version_commit!())]
/// Golem network server.
///
/// By running this software you declare that you have read,
/// understood and hereby accept the disclaimer and
/// privacy warning found at https://handbook.golem.network/see-also/terms
///
struct CliArgs {
    /// Service data dir
    #[structopt(
        short,
        long = "datadir",
        set = clap::ArgSettings::Global,
        env = "YAGNA_DATADIR",
        default_value = &*DEFAULT_DATA_DIR,
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

    /// Accept the disclaimer and privacy warning found at
    /// {n}https://handbook.golem.network/see-also/terms
    #[structopt(long, set = clap::ArgSettings::Global)]
    accept_terms: bool,

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
            accept_terms: args.accept_terms,
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
}

#[allow(unused)]
async fn start_payment_drivers(data_dir: &Path) -> anyhow::Result<()> {
    #[cfg(feature = "dummy-driver")]
    {
        use ya_dummy_driver::PaymentDriverService;
        PaymentDriverService::gsb(&()).await?;
    }
    #[cfg(feature = "gnt-driver")]
    {
        use ya_gnt_driver::PaymentDriverService;
        let db_executor = DbExecutor::from_data_dir(data_dir, "gnt-driver")?;
        PaymentDriverService::gsb(&db_executor).await?;
    }
    Ok(())
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
        if !ctx.accept_terms {
            prompt_terms()?;
        }
        match self {
            Self::Run(ServiceCommandOpts { api_url }) => {
                let name = clap::crate_name!();
                log::info!("Starting {} service!", name);

                let stats = Statistics::new()?;

                ya_sb_router::bind_gsb_router(ctx.gsb_url.clone())
                    .await
                    .context("binding service bus router")?;

                let context = ServiceContext::from_data_dir(&ctx.data_dir, name)?;
                Services::gsb(&context).await?;
                start_payment_drivers(&ctx.data_dir).await?;

                payment_accounts::save_default_account()
                    .await
                    .unwrap_or_else(|e| {
                        log::error!("Saving default payment account failed: {}", e)
                    });
                payment_accounts::init_accounts()
                    .await
                    .unwrap_or_else(|e| log::error!("Initializing payment accounts failed: {}", e));

                let api_host_port = rest_api_host_port(api_url.clone());
                HttpServer::new(move || {
                    let stats_rest = actix_web::web::scope("v1/")
                        .data(stats.clone())
                        .service(expose_metrics);

                    let app = App::new()
                        .wrap(middleware::Logger::default())
                        .wrap(auth::Auth::default())
                        .route("/me", web::get().to(me))
                        .service(stats_rest);

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

fn prompt_terms() -> Result<()> {
    use std::io::Write;

    let header = r#"
By running this software you declare that you have read, understood
and hereby accept the disclaimer and privacy warning found at
https://handbook.golem.network/see-also/terms

"#;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    stdout.write(header.as_bytes())?;
    stdout.flush()?;

    loop {
        stdout.write("Do you accept the terms and conditions? [yes/no]: ".as_bytes())?;
        stdout.flush()?;

        let mut buffer = String::new();
        stdin.read_line(&mut buffer)?;
        match buffer.to_lowercase().trim() {
            "yes" => return Ok(()),
            "no" => std::process::exit(1),
            _ => (),
        }
    }
}

async fn me(id: Identity) -> impl Responder {
    web::Json(id)
}

#[actix_web::get("/metrics")]
pub async fn expose_metrics(stats_holder: web::Data<Arc<Mutex<Statistics>>>) -> impl Responder {
    let metrics = stats_holder.lock().await.query_metrics();
    //info!("{}", metrics);

    return metrics;
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
