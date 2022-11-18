use actix::{Actor, Addr};
use anyhow::{bail, Context};
use futures::channel::oneshot;
use std::convert::TryFrom;
use std::path::PathBuf;
use structopt::{clap, StructOpt};

use ya_client_model::activity::ExeScriptCommand;
use ya_service_bus::RpcEnvelope;

use ya_core_model::activity;
use ya_exe_unit::agreement::Agreement;
use ya_exe_unit::logger::*;
use ya_exe_unit::manifest::ManifestContext;
use ya_exe_unit::message::{GetState, GetStateResponse, Register};
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::service::metrics::MetricsService;
use ya_exe_unit::service::signal::SignalMonitor;
use ya_exe_unit::service::transfer::TransferService;
use ya_exe_unit::state::Supervision;
use ya_exe_unit::{ExeUnit, ExeUnitContext};
use ya_utils_path::normalize_path;

#[derive(structopt::StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
struct Cli {
    /// Runtime binary path
    #[structopt(long, short)]
    binary: PathBuf,
    #[structopt(flatten)]
    supervise: SuperviseCli,
    /// Additional runtime arguments
    #[structopt(
        long,
        short,
        set = clap::ArgSettings::Global,
        number_of_values = 1,
    )]
    runtime_arg: Vec<String>,
    /// Enclave secret key used in secure communication
    #[structopt(
        long,
        env = "EXE_UNIT_SEC_KEY",
        hide_env_values = true,
        set = clap::ArgSettings::Global,
    )]
    #[allow(dead_code)]
    sec_key: Option<String>,
    /// Requestor public key used in secure communication
    #[structopt(
        long,
        env = "EXE_UNIT_REQUESTOR_PUB_KEY",
        hide_env_values = true,
        set = clap::ArgSettings::Global,
    )]
    #[allow(dead_code)]
    requestor_pub_key: Option<String>,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(structopt::StructOpt, Debug)]
struct SuperviseCli {
    /// Hardware resources are handled by the runtime
    #[structopt(
        long = "runtime-managed-hardware",
        alias = "cap-handoff",
        parse(from_flag = std::ops::Not::not),
        set = clap::ArgSettings::Global,
    )]
    hardware: bool,
    /// Images are handled by the runtime
    #[structopt(
        long = "runtime-managed-image",
        parse(from_flag = std::ops::Not::not),
        set = clap::ArgSettings::Global,
    )]
    image: bool,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
enum Command {
    /// Execute commands from file
    FromFile {
        /// ExeUnit daemon GSB URL
        #[structopt(long)]
        report_url: Option<String>,
        /// ExeUnit service ID
        #[structopt(long)]
        service_id: Option<String>,
        /// Command file path
        input: PathBuf,
        #[structopt(flatten)]
        args: RunArgs,
    },
    /// Bind to Service Bus
    ServiceBus {
        /// ExeUnit service ID
        service_id: String,
        /// ExeUnit daemon GSB URL
        report_url: String,
        #[structopt(flatten)]
        args: RunArgs,
    },
    /// Print an offer template in JSON format
    OfferTemplate,
}

#[derive(structopt::StructOpt, Debug)]
struct RunArgs {
    /// Agreement file path
    #[structopt(long, short)]
    agreement: PathBuf,
    /// Working directory
    #[structopt(long, short)]
    work_dir: PathBuf,
    /// Common cache directory
    #[structopt(long, short)]
    cache_dir: PathBuf,
}

fn create_path(path: &PathBuf) -> anyhow::Result<PathBuf> {
    if let Err(error) = std::fs::create_dir_all(path) {
        match &error.kind() {
            std::io::ErrorKind::AlreadyExists => (),
            _ => bail!("Can't create directory: {}, {}", path.display(), error),
        }
    }
    Ok(normalize_path(path)?)
}

#[cfg(feature = "sgx")]
fn init_crypto(
    sec_key: Option<String>,
    req_key: Option<String>,
) -> anyhow::Result<ya_exe_unit::crypto::Crypto> {
    use ya_exe_unit::crypto::Crypto;

    let req_key = req_key.ok_or_else(|| anyhow::anyhow!("Missing requestor public key"))?;
    match sec_key {
        Some(key) => Ok(Crypto::try_with_keys(key, req_key)?),
        None => {
            log::info!("Generating a new key pair...");
            Ok(Crypto::try_new(req_key)?)
        }
    }
}

async fn send_script(
    exe_unit: Addr<ExeUnit<RuntimeProcess>>,
    activity_id: Option<String>,
    exe_script: Vec<ExeScriptCommand>,
) {
    use std::time::Duration;
    use ya_exe_unit::state::{State, StatePair};

    let delay = Duration::from_secs_f32(0.5);
    loop {
        match exe_unit.send(GetState).await {
            Ok(GetStateResponse(StatePair(State::Initialized, None))) => break,
            Ok(GetStateResponse(StatePair(State::Terminated, _)))
            | Ok(GetStateResponse(StatePair(_, Some(State::Terminated))))
            | Err(_) => {
                return log::error!("ExeUnit has terminated");
            }
            _ => tokio::time::sleep(delay).await,
        }
    }

    log::debug!("Executing commands: {:?}", exe_script);

    let msg = activity::Exec {
        activity_id: activity_id.unwrap_or_default(),
        batch_id: hex::encode(&rand::random::<[u8; 16]>()),
        exe_script,
        timeout: None,
    };
    if let Err(e) = exe_unit
        .send(RpcEnvelope::with_caller(String::new(), msg))
        .await
    {
        log::error!("Unable to execute exe script: {:?}", e);
    }
}

#[cfg(feature = "packet-trace-enable")]
fn init_packet_trace() -> anyhow::Result<()> {
    use ya_packet_trace::{set_write_target, WriteTarget};

    let write = std::fs::File::create("/home/kamil/exe-unit.trace")?;
    set_write_target(WriteTarget::Write(Box::new(write)));

    Ok(())
}

async fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    #[cfg(feature = "packet-trace-enable")]
    init_packet_trace()?;

    #[allow(unused_mut)]
    let mut cli: Cli = Cli::from_args();
    if !cli.binary.exists() {
        bail!("Runtime binary does not exist: {}", cli.binary.display());
    }

    let mut commands = None;
    let ctx_activity_id;
    let ctx_report_url;

    let args = match &cli.command {
        Command::FromFile {
            args,
            service_id,
            report_url,
            input,
        } => {
            let contents = std::fs::read_to_string(input).map_err(|e| {
                anyhow::anyhow!("Cannot read commands from file {}: {e}", input.display())
            })?;
            let contents = serde_json::from_str(&contents).map_err(|e| {
                anyhow::anyhow!(
                    "Cannot deserialize commands from file {}: {e}",
                    input.display(),
                )
            })?;
            ctx_activity_id = service_id.clone();
            ctx_report_url = report_url.clone();
            commands = Some(contents);
            args
        }
        Command::ServiceBus {
            args,
            service_id,
            report_url,
        } => {
            ctx_activity_id = Some(service_id.clone());
            ctx_report_url = Some(report_url.clone());
            args
        }
        Command::OfferTemplate => {
            let args = cli.runtime_arg.clone();
            let offer_template = ExeUnit::<RuntimeProcess>::offer_template(cli.binary, args)?;
            println!("{}", serde_json::to_string(&offer_template)?);
            return Ok(());
        }
    };

    if !args.agreement.exists() {
        bail!(
            "Agreement file does not exist: {}",
            args.agreement.display()
        );
    }
    let work_dir = create_path(&args.work_dir).map_err(|e| {
        anyhow::anyhow!(
            "Cannot create the working directory {}: {e}",
            args.work_dir.display(),
        )
    })?;
    let cache_dir = create_path(&args.cache_dir).map_err(|e| {
        anyhow::anyhow!(
            "Cannot create the cache directory {}: {e}",
            args.work_dir.display(),
        )
    })?;
    let mut agreement = Agreement::try_from(&args.agreement).map_err(|e| {
        anyhow::anyhow!(
            "Error parsing the agreement from {}: {e}",
            args.agreement.display(),
        )
    })?;

    log::info!("Attempting to read app manifest ..");

    let manifest_ctx =
        ManifestContext::try_new(&agreement.inner).context("Invalid app manifest")?;
    agreement.task_package = manifest_ctx
        .payload()
        .or_else(|| agreement.task_package.take());

    log::info!("Manifest-enabled features: {:?}", manifest_ctx.features());
    log::info!("User-provided payload: {:?}", agreement.task_package);

    let ctx = ExeUnitContext {
        supervise: Supervision {
            hardware: cli.supervise.hardware,
            image: cli.supervise.image,
            manifest: manifest_ctx,
        },
        activity_id: ctx_activity_id.clone(),
        report_url: ctx_report_url,
        agreement,
        work_dir,
        cache_dir,
        runtime_args: cli.runtime_arg.clone(),
        acl: Default::default(),
        credentials: None,
        #[cfg(feature = "sgx")]
        crypto: init_crypto(
            cli.sec_key.replace("<hidden>".into()),
            cli.requestor_pub_key.clone(),
        )?,
    };

    log::debug!("CLI args: {:?}", cli);
    log::debug!("ExeUnitContext args: {:?}", ctx);

    let (tx, rx) = oneshot::channel();

    let metrics = MetricsService::try_new(&ctx, Some(10000), ctx.supervise.hardware)?.start();
    let transfers = TransferService::new(&ctx).start();
    let runtime = RuntimeProcess::new(&ctx, cli.binary).start();
    let exe_unit = ExeUnit::new(tx, ctx, metrics, transfers, runtime).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();
    exe_unit.send(Register(signals)).await?;

    if let Some(exe_script) = commands {
        tokio::task::spawn(send_script(exe_unit, ctx_activity_id, exe_script));
    }

    rx.await??;
    Ok(())
}

#[actix_rt::main]
async fn main() {
    let panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |e| {
        log::error!("ExeUnit Supervisor panic: {e}");
        panic_hook(e)
    }));

    if let Err(error) = start_file_logger() {
        start_logger().expect("Failed to start logging");
        log::warn!("Using fallback logging due to an error: {:?}", error);
    };

    std::process::exit(match run().await {
        Ok(_) => 0,
        Err(error) => {
            log::error!("{}", error);
            1
        }
    })
}
