#![allow(clippy::obfuscated_if_else)]

use actix_web::{middleware, web, App, HttpServer, Responder};
use anyhow::{Context, Result};
use futures::prelude::*;
use metrics::gauge;
#[cfg(feature = "static-openssl")]
extern crate openssl_probe;

use std::sync::Arc;
use std::{
    any::TypeId,
    collections::HashMap,
    convert::{TryFrom, TryInto},
    env,
    fmt::Debug,
    path::{Path, PathBuf},
    time::Duration,
};
use structopt::{clap, StructOpt};
use url::Url;
use ya_activity::service::Activity as ActivityService;
use ya_file_logging::start_logger;
use ya_gsb_api::GsbApiService;
use ya_identity::service::Identity as IdentityService;
use ya_market::MarketService;
use ya_metrics::{MetricsPusherOpts, MetricsService};
use ya_net::Net as NetService;
use ya_payment::{accounts as payment_accounts, PaymentService};
use ya_persistence::executor::{DbExecutor, DbMixedExecutor};
use ya_persistence::service::Persistence as PersistenceService;
use ya_sb_proto::{DEFAULT_GSB_URL, GSB_URL_ENV_VAR};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_api_interfaces::Provider;
use ya_service_api_web::{
    middleware::{auth, cors::CorsConfig, Identity},
    rest_api_host_port, DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR,
};
use ya_sgx::SgxService;
use ya_utils_path::data_dir::DataDir;
use ya_utils_process::lock::ProcLock;
use ya_version::VersionService;
use ya_vpn::VpnService;

use ya_service_bus::typed as gsb;

mod autocomplete;
mod extension;
mod model;

use crate::extension::Extension;
use autocomplete::CompleteCommand;

use ya_activity::TrackerRef;
use ya_service_api_web::middleware::cors::AppKeyCors;

lazy_static::lazy_static! {
    static ref DEFAULT_DATA_DIR: String = DataDir::new(clap::crate_name!()).to_string();
}

const FD_METRICS_INTERVAL: Duration = Duration::from_secs(60);

#[derive(StructOpt, Debug)]
#[structopt(about = clap::crate_description!())]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
/// Golem network server.
///
/// By running this software you declare that you have read,
/// understood and hereby accept the disclaimer and
/// privacy warning found at https://docs.golem.network/docs/golem/terms
///
/// Use RUST_LOG env variable to change log level.
struct CliArgs {
    /// Accept the disclaimer and privacy warning found at
    /// {n}https://docs.golem.network/docs/golem/terms
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

    #[structopt(hidden = true)]
    #[structopt(long, set = clap::ArgSettings::Global)]
    quiet: bool,

    #[structopt(subcommand)]
    command: CliCommand,
}

impl CliArgs {
    pub fn get_data_dir(&self) -> Result<PathBuf> {
        self.data_dir.get_or_create()
    }

    pub async fn run_command(self) -> Result<()> {
        let ctx: CliCtx = (&self).try_into()?;

        ctx.output(self.command.run_command(&ctx).await?)?;
        Ok(())
    }
}

impl TryFrom<&CliArgs> for CliCtx {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> Result<Self, Self::Error> {
        let data_dir = args.get_data_dir()?;

        Ok(CliCtx {
            data_dir,
            gsb_url: Some(args.gsb_url.clone()),
            json_output: args.json,
            quiet: args.quiet,
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
    mixed_dbs: HashMap<TypeId, DbMixedExecutor>,
    default_db: DbExecutor,
    default_mixed: DbMixedExecutor,
    activity_tracker: ya_activity::TrackerRef,
}

impl<S: 'static> Provider<S, DbExecutor> for ServiceContext {
    fn component(&self) -> DbExecutor {
        match self.dbs.get(&TypeId::of::<S>()) {
            Some(db) => db.clone(),
            None => self.default_db.clone(),
        }
    }
}

impl<S: 'static> Provider<S, DbMixedExecutor> for ServiceContext {
    fn component(&self) -> DbMixedExecutor {
        match self.mixed_dbs.get(&TypeId::of::<S>()) {
            Some(db) => db.clone(),
            None => self.default_mixed.clone(),
        }
    }
}

impl<S: 'static> Provider<S, ya_activity::TrackerRef> for ServiceContext {
    fn component(&self) -> ya_activity::TrackerRef {
        self.activity_tracker.clone()
    }
}

impl<S: 'static> Provider<S, CliCtx> for ServiceContext {
    fn component(&self) -> CliCtx {
        self.ctx.clone()
    }
}

impl<S: 'static> Provider<S, ()> for ServiceContext {
    fn component(&self) {}
}

impl ServiceContext {
    fn make_entry<S: 'static>(path: &Path, name: &str) -> Result<(TypeId, DbExecutor)> {
        Ok((TypeId::of::<S>(), DbExecutor::from_data_dir(path, name)?))
    }

    fn make_mixed_entry<S: 'static>(path: &Path, name: &str) -> Result<(TypeId, DbMixedExecutor)> {
        let disk_db = DbExecutor::from_data_dir(path, name)?;
        let ram_db = DbExecutor::in_memory(name)?;

        Ok((TypeId::of::<S>(), DbMixedExecutor::new(disk_db, ram_db)))
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
            Self::make_entry::<ActivityService>(&ctx.data_dir, "activity")?,
            Self::make_entry::<PaymentService>(&ctx.data_dir, "payment")?,
        ]
        .iter()
        .cloned()
        .collect();

        let market_db = Self::make_mixed_entry::<MarketService>(&ctx.data_dir, "market")?;
        let mixed_dbs = [market_db.clone()].iter().cloned().collect();
        let activity_tracker = TrackerRef::create();

        Ok(ServiceContext {
            ctx,
            dbs,
            mixed_dbs,
            default_db,
            default_mixed: market_db.1,
            activity_tracker,
        })
    }
}

#[ya_service_api_derive::services(ServiceContext)]
enum Services {
    #[enable(gsb, cli)]
    Db(PersistenceService),
    // Metrics service must be activated before all other services
    // to that will use it. Identity service is used by the Metrics,
    // so must be initialized before.
    #[enable(gsb, cli(flatten))]
    Identity(IdentityService),
    #[enable(gsb, rest)]
    Metrics(MetricsService),
    #[enable(gsb, rest, cli)]
    Version(VersionService),
    #[enable(gsb, rest, cli)]
    Net(NetService),
    //TODO enable VpnService::rest for v2 / or create common scope for v1 and v2
    #[enable(rest)]
    Vpn(VpnService),
    #[enable(gsb, rest, cli)]
    Market(MarketService),
    #[enable(gsb, rest, cli)]
    Activity(ActivityService),
    #[enable(gsb, rest, cli)]
    Payment(PaymentService),
    #[enable(gsb)]
    SgxDriver(SgxService),
    #[enable(gsb, rest)]
    GsbApi(GsbApiService),
}

#[cfg(not(any(feature = "dummy-driver", feature = "erc20next-driver",)))]
compile_error!("At least one payment driver needs to be enabled in order to make payments.");

#[allow(unused)]
async fn start_payment_drivers(data_dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut drivers = vec![];
    #[cfg(feature = "dummy-driver")]
    {
        use ya_dummy_driver::{PaymentDriverService, DRIVER_NAME};
        PaymentDriverService::gsb(&()).await?;
        drivers.push(DRIVER_NAME.to_owned());
    }
    #[cfg(feature = "erc20next-driver")]
    {
        use ya_erc20next_driver::{PaymentDriverService, DRIVER_NAME};
        PaymentDriverService::gsb(data_dir.to_path_buf()).await?;
        drivers.push(DRIVER_NAME.to_owned());
    }
    Ok(drivers)
}

#[allow(clippy::large_enum_variant)]
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

    /// Extension management
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Extension(ExtensionCommand),

    #[structopt(external_subcommand)]
    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    Other(Vec<String>),
}

impl CliCommand {
    pub async fn run_command(self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            CliCommand::Commands(command) => {
                start_logger("warn", None, &[], false)?;
                command.run_command(ctx).await
            }
            CliCommand::Complete(complete) => complete.run_command(ctx),
            CliCommand::Service(service) => service.run_command(ctx).await,
            CliCommand::Extension(ext) => ext.run_command(ctx).await,
            CliCommand::Other(args) => extension::run::<CliArgs>(ctx, args).await,
        }
    }
}

#[derive(StructOpt, Debug)]
enum ExtensionCommand {
    /// List available extensions
    List {},
    /// Autostart extension
    Register { args: Vec<String> },
    /// Remove extension from autostart
    Unregister { name: String },
}

impl ExtensionCommand {
    pub async fn run_command(self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            ExtensionCommand::List {} => {
                let extensions = Extension::list();

                if ctx.json_output {
                    Self::map(extensions.into_iter())
                } else {
                    Self::table(extensions.into_iter())
                }
            }
            ExtensionCommand::Register { mut args } => {
                let mut ext = Extension::find(args.clone())?;
                args.remove(0);

                ext.conf.args = args;
                ext.conf.autostart = true;
                ext.write_conf().await?;

                Ok(CommandOutput::NoOutput)
            }
            ExtensionCommand::Unregister { name } => {
                let mut ext = Extension::find(vec![name])?;
                ext.conf.autostart = false;
                ext.write_conf().await?;

                Ok(CommandOutput::NoOutput)
            }
        }
    }

    fn map<I: Iterator<Item = Extension>>(extensions: I) -> Result<CommandOutput> {
        CommandOutput::object(
            extensions
                .map(|mut ext| {
                    let name = std::mem::take(&mut ext.name);
                    (name, ext)
                })
                .collect::<HashMap<_, _>>(),
        )
    }

    fn table<I: Iterator<Item = Extension>>(extensions: I) -> Result<CommandOutput> {
        Ok(ResponseTable {
            columns: vec![
                "name".into(),
                "autostart".into(),
                "path".into(),
                "args".into(),
            ],
            values: extensions
                .map(|ext| {
                    serde_json::json! {[
                        ext.name,
                        if ext.conf.autostart { 'x' } else { ' ' },
                        ext.path,
                        ext.conf.args.join(" "),
                    ]}
                })
                .collect(),
        }
        .into())
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(StructOpt, Debug)]
enum ServiceCommand {
    /// Runs server in foreground
    Run(ServiceCommandOpts),
    Shutdown(ShutdownOpts),
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

    #[structopt(long, env, default_value = "60")]
    max_rest_timeout: u64,

    ///changes log level from info to debug
    #[structopt(long)]
    debug: bool,

    /// Create logs in this directory. Logs are automatically rotated and compressed.
    /// If unset, then `data_dir` is used.
    /// If set to empty string, then logging to files is disabled.
    #[structopt(long, env = "YAGNA_LOG_DIR")]
    log_dir: Option<PathBuf>,

    #[structopt(flatten)]
    cors: CorsConfig,
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
    let socket = tokio::net::UnixDatagram::unbound()?;
    socket.send_to(state.as_ref(), addr).await?;
    Ok(())
}

#[derive(StructOpt, Debug)]
struct ShutdownOpts {
    #[structopt(long)]
    gracefully: bool,
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
                max_rest_timeout,
                log_dir,
                debug,
                cors,
            }) => {
                // workaround to silence middleware logger by default
                // to enable it explicitly set RUST_LOG=info or more verbose
                env::set_var(
                    "RUST_LOG",
                    env::var("RUST_LOG").unwrap_or_else(|_| {
                        "info,actix_web::middleware::logger=warn,sqlx=warn".to_string()
                    }),
                );

                //this force_debug flag sets default log level to debug
                //if the --debug option is set
                let force_debug = *debug;
                let logger_handle = start_logger(
                    "info",
                    log_dir.as_deref().or(Some(&ctx.data_dir)).and_then(|path| {
                        match path.components().count() {
                            0 => None,
                            _ => Some(path),
                        }
                    }),
                    &vec![
                        ("actix_http::response", log::LevelFilter::Off),
                        ("h2", log::LevelFilter::Off),
                        ("hyper", log::LevelFilter::Info),
                        ("reqwest", log::LevelFilter::Info),
                        ("tokio_core", log::LevelFilter::Info),
                        ("tokio_reactor", log::LevelFilter::Info),
                        ("trust_dns_resolver", log::LevelFilter::Info),
                        ("trust_dns_proto", log::LevelFilter::Info),
                        ("web3", log::LevelFilter::Info),
                        ("tokio_util", log::LevelFilter::Off),
                        ("mio", log::LevelFilter::Off),
                    ],
                    force_debug,
                )?;

                let app_name = clap::crate_name!();
                log::info!(
                    "Starting {} service! Version: {}.",
                    app_name,
                    ya_compile_time_utils::version_describe!()
                );
                log::info!("Data directory: {}", ctx.data_dir.display());

                let _lock = ProcLock::new(app_name, &ctx.data_dir)?.lock(std::process::id())?;

                ya_sb_router::bind_gsb_router(ctx.gsb_url.clone())
                    .await
                    .context("binding service bus router")?;

                let mut context: ServiceContext = ctx.clone().try_into()?;
                context.set_metrics_ctx(metrics_opts);
                Services::gsb(&context).await?;

                ya_compile_time_utils::report_version_to_metrics();

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
                let rest_address = api_host_port.clone();
                let cors = AppKeyCors::new(cors).await?;

                tokio::task::spawn_local(async move {
                    ya_net::hybrid::send_bcast_new_neighbour().await
                });

                let number_of_workers = env::var("YAGNA_HTTP_WORKERS")
                    .ok()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or_else(num_cpus::get)
                    .clamp(1, 256);
                let count_started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let server = HttpServer::new(move || {
                    let app = App::new()
                        .wrap(middleware::Logger::default())
                        .wrap(auth::Auth::new(cors.cache()))
                        .wrap(cors.cors())
                        .route("/me", web::get().to(me))
                        .service(forward_gsb);
                    let rest = Services::rest(app, &context);
                    if count_started.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        == number_of_workers - 1
                    {
                        log::info!(
                            "All {} http workers started - listening on {}",
                            number_of_workers,
                            rest_address
                        );
                    }
                    rest
                })
                .workers(number_of_workers)
                // this is maximum supported timeout for our REST API
                .keep_alive(std::time::Duration::from_secs(*max_rest_timeout))
                .bind(api_host_port.clone())
                .context(format!("Failed to bind http server on {:?}", api_host_port))?
                .run();

                let _ = extension::autostart(&ctx.data_dir, api_url, &ctx.gsb_url)
                    .await
                    .map_err(|e| log::warn!("Failed to autostart extensions: {e}"));

                {
                    let server_handle = server.handle();
                    gsb::bind(model::BUS_ID, move |request: model::ShutdownRequest| {
                        log::info!(
                            "ShutdownRequest {}",
                            request.graceful.then_some("graceful").unwrap_or("")
                        );
                        let server_handle = server_handle.clone();
                        async move {
                            server_handle.stop(request.graceful).await;
                            Ok(())
                        }
                    });
                }

                tokio::spawn(async {
                    loop {
                        for (fd_type, count) in ya_fd_metrics::fd_metrics() {
                            gauge!(format!("yagna.fds.{fd_type}"), count as i64);
                        }

                        tokio::time::sleep(FD_METRICS_INTERVAL).await;
                    }
                });

                future::try_join(server, sd_notify(false, "READY=1")).await?;

                log::info!("{} service successfully finished!", app_name);

                PaymentService::shut_down().await;
                NetService::shutdown()
                    .await
                    .map_err(|e| log::error!("Error shutting down NET: {}", e))
                    .ok();

                logger_handle.shutdown();
                Ok(CommandOutput::NoOutput)
            }
            Self::Shutdown(opts) => {
                let result = gsb::service(model::BUS_ID)
                    .call(model::ShutdownRequest {
                        graceful: opts.gracefully,
                    })
                    .await?;
                CommandOutput::object(result)
            }
        }
    }
}

fn prompt_terms() -> Result<()> {
    use std::io::Write;

    let header = r#"
By running this software you declare that you have read, understood
and hereby accept the disclaimer and privacy warning found at
https://docs.golem.network/docs/golem/terms

"#;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    let _ = stdout.write(header.as_bytes())?;
    stdout.flush()?;

    loop {
        let _ = stdout.write("Do you accept the terms and conditions? [yes/no]: ".as_bytes())?;
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

#[actix_web::post("/_gsb/{service:.*}")]
async fn forward_gsb(
    id: Identity,
    service: web::Path<String>,
    data: web::Json<serde_json::Value>,
) -> impl Responder {
    use ya_service_bus::untyped as bus;
    let service = service.into_inner();

    log::debug!(target: "gsb-bridge", "called: {}", service);

    let inner_data = data.into_inner();
    let data = ya_service_bus::serialization::to_vec(&inner_data)
        .map_err(actix_web::error::ErrorBadRequest)?;
    let r = bus::send(
        &format!("/{}", service),
        &format!("/local/{}", id.identity),
        &data,
    )
    .await
    .map_err(actix_web::error::ErrorInternalServerError)?;

    let json_resp: serde_json::Value = ya_service_bus::serialization::from_slice(&r)
        .map_err(actix_web::error::ErrorInternalServerError)?;
    Ok::<_, actix_web::Error>(web::Json(json_resp))
}

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    #[cfg(feature = "static-openssl")]
    openssl_probe::init_ssl_cert_env_vars();
    let args = CliArgs::from_args();

    std::env::set_var(GSB_URL_ENV_VAR, args.gsb_url.as_str()); // FIXME

    match args.run_command().await {
        Ok(()) => Ok(()),
        Err(err) => {
            //this way runtime/command error is at least possibly visible in yagna logs
            log::error!("Exiting..., error details: {:?}", err);
            Err(err)
        }
    }
}
