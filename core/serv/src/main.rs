use actix_web::{middleware, web, App, HttpServer, Responder};
use anyhow::{Context, Result};
use futures::prelude::*;
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
use ya_activity::service::Activity as ActivityService;
use ya_identity::service::Identity as IdentityService;
use ya_market::MarketService;
use ya_metrics::{MetricsPusherOpts, MetricsService};
use ya_net::Net as NetService;
use ya_payment::{accounts as payment_accounts, PaymentService};
use ya_sgx::SgxService;

use ya_persistence::executor::DbExecutor;
use ya_sb_proto::{DEFAULT_GSB_URL, GSB_URL_ENV_VAR};
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_interfaces::Provider;
use ya_service_api_web::{
    middleware::{auth, Identity},
    rest_api_host_port, DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR,
};
use ya_utils_path::data_dir::DataDir;
use ya_utils_process::lock::ProcLock;

mod autocomplete;
use autocomplete::CompleteCommand;

lazy_static::lazy_static! {
    static ref DEFAULT_DATA_DIR: String = DataDir::new(clap::crate_name!()).to_string();
}

#[derive(StructOpt, Debug)]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
/// Golem network server.
///
/// By running this software you declare that you have read,
/// understood and hereby accept the disclaimer and
/// privacy warning found at https://handbook.golem.network/see-also/terms
///
struct CliArgs {
    /// Accept the disclaimer and privacy warning found at
    /// {n}https://handbook.golem.network/see-also/terms
    #[structopt(long)]
    #[cfg_attr(not(feature = "tos"), structopt(hidden = true))]
    accept_terms: bool,

    /// Service data dir
    #[structopt(
        short,
        long = "datadir",
        env = "YAGNA_DATADIR",
        default_value = &*DEFAULT_DATA_DIR,
        hide_env_values = true,
        set = clap::ArgSettings::Global,
    )]
    data_dir: DataDir,

    /// Service Bus (aka GSB) URL
    #[structopt(
        short,
        long,
        env = GSB_URL_ENV_VAR,
        default_value = DEFAULT_GSB_URL,
        hide_env_values = true,
        set = clap::ArgSettings::Global,
    )]
    gsb_url: Url,

    /// Return results in JSON format
    #[structopt(long, set = clap::ArgSettings::Global)]
    json: bool,

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
            accept_terms: if cfg!(feature = "tos") {
                args.accept_terms
            } else {
                true
            },
            metrics_ctx: None,
        })
    }
}

#[derive(Clone)]
struct ServiceContext {
    ctx: CliCtx,
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

impl<S: 'static> Provider<S, CliCtx> for ServiceContext {
    fn component(&self) -> CliCtx {
        self.ctx.clone()
    }
}

impl<S: 'static> Provider<S, ()> for ServiceContext {
    fn component(&self) -> () {
        ()
    }
}

impl ServiceContext {
    fn make_entry<S: 'static>(path: &PathBuf, name: &str) -> Result<(TypeId, DbExecutor)> {
        Ok((TypeId::of::<S>(), DbExecutor::from_data_dir(path, name)?))
    }

    fn set_metrics_ctx(&mut self, metrics_opts: &MetricsPusherOpts) {
        self.ctx.metrics_ctx = Some(metrics_opts.into())
    }
}

impl TryFrom<CliCtx> for ServiceContext {
    type Error = anyhow::Error;

    fn try_from(ctx: CliCtx) -> Result<Self, Self::Error> {
        let default_name = clap::crate_name!();
        let default_db = DbExecutor::from_data_dir(&ctx.data_dir, default_name)?;
        let dbs = [
            Self::make_entry::<MarketService>(&ctx.data_dir, "market")?,
            Self::make_entry::<ActivityService>(&ctx.data_dir, "activity")?,
            Self::make_entry::<PaymentService>(&ctx.data_dir, "payment")?,
        ]
        .iter()
        .cloned()
        .collect();

        Ok(ServiceContext {
            ctx,
            dbs,
            default_db,
        })
    }
}

#[ya_service_api_derive::services(ServiceContext)]
enum Services {
    // Metrics service must be activated first, to allow all
    // other services to initialize counters and other metrics.
    #[enable(gsb, rest)]
    Metrics(MetricsService),
    #[enable(gsb, cli(flatten))]
    Identity(IdentityService),
    #[enable(gsb)]
    Net(NetService),
    #[enable(gsb, rest)]
    Market(MarketService),
    #[enable(gsb, rest, cli)]
    Activity(ActivityService),
    #[enable(gsb, rest, cli)]
    Payment(PaymentService),
    #[enable(gsb)]
    SgxDriver(SgxService),
}

#[allow(unused)]
async fn start_payment_drivers(data_dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut drivers = vec![];
    #[cfg(feature = "dummy-driver")]
    {
        use ya_dummy_driver::{PaymentDriverService, DRIVER_NAME};
        PaymentDriverService::gsb(&()).await?;
        drivers.push(DRIVER_NAME.to_owned());
    }
    #[cfg(feature = "gnt-driver")]
    {
        use ya_gnt_driver::{PaymentDriverService, DRIVER_NAME};
        let db_executor = DbExecutor::from_data_dir(data_dir, "gnt-driver")?;
        PaymentDriverService::gsb(&db_executor).await?;
        drivers.push(DRIVER_NAME.to_owned());
    }
    #[cfg(feature = "zksync-driver")]
    {
        use ya_zksync_driver::{PaymentDriverService, DRIVER_NAME};
        let db_executor = DbExecutor::from_data_dir(data_dir, "zksync-driver")?;
        PaymentDriverService::gsb(&db_executor).await?;
        drivers.push(DRIVER_NAME.to_owned());
    }
    Ok(drivers)
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

    #[structopt(flatten)]
    metrics_opts: MetricsPusherOpts,
}

#[cfg(unix)]
async fn sd_notify(unset_environment: bool, state: &str) -> std::io::Result<()> {
    let addr = match env::var_os("NOTIFY_SOCKET") {
        Some(v) => v,
        None => {
            return Ok(());
        }
    };
    if unset_environment {
        env::remove_var("NOTIFY_SOCKET");
    }
    let mut socket = tokio::net::UnixDatagram::unbound()?;
    socket.send_to(state.as_ref(), addr).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn sd_notify(_unset_environment: bool, _state: &str) -> std::io::Result<()> {
    // ignore for windows.
    Ok(())
}

impl ServiceCommand {
    async fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        if !ctx.accept_terms {
            prompt_terms()?;
        }
        match self {
            Self::Run(ServiceCommandOpts {
                api_url,
                metrics_opts,
            }) => {
                log::info!(
                    "Starting {} service! Version: {}.",
                    clap::crate_name!(),
                    ya_compile_time_utils::version_describe!()
                );
                let _lock = ProcLock::new("yagna", &ctx.data_dir)?.lock(std::process::id())?;

                ya_sb_router::bind_gsb_router(ctx.gsb_url.clone())
                    .await
                    .context("binding service bus router")?;

                let mut context: ServiceContext = ctx.clone().try_into()?;
                context.set_metrics_ctx(metrics_opts);
                Services::gsb(&context).await?;
                let drivers = start_payment_drivers(&ctx.data_dir).await?;

                payment_accounts::save_default_account(&ctx.data_dir, drivers)
                    .await
                    .unwrap_or_else(|e| {
                        log::error!("Saving default payment account failed: {}", e)
                    });
                payment_accounts::init_accounts(&ctx.data_dir)
                    .await
                    .unwrap_or_else(|e| log::error!("Initializing payment accounts failed: {}", e));

                let api_host_port = rest_api_host_port(api_url.clone());

                let server = HttpServer::new(move || {
                    let app = App::new()
                        .wrap(middleware::Logger::default())
                        .wrap(auth::Auth::default())
                        .route("/me", web::get().to(me));

                    Services::rest(app, &context)
                })
                .bind(api_host_port.clone())
                .context(format!("Failed to bind http server on {:?}", api_host_port))?;

                future::try_join(server.run(), sd_notify(false, "READY=1")).await?;

                log::info!("{} daemon successfully finished!", clap::crate_name!());
                Ok(CommandOutput::NoOutput)
            }
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

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let args: CliArgs = CliArgs::from_args();

    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or(format!(
            "{},actix_web::middleware::logger=warn",
            args.log_level()
        )),
    );
    env_logger::builder()
        .filter_module("actix_http::response", log::LevelFilter::Off)
        .filter_module("tokio_core", log::LevelFilter::Info)
        .filter_module("tokio_reactor", log::LevelFilter::Info)
        .filter_module("hyper", log::LevelFilter::Info)
        .filter_module("web3", log::LevelFilter::Info)
        .init();

    std::env::set_var(GSB_URL_ENV_VAR, args.gsb_url.as_str()); // FIXME

    args.run_command().await
}
