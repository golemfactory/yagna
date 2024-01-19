#[macro_use]
extern crate derive_more;

use actix::prelude::*;
use anyhow::{bail, Context};
use std::convert::TryFrom;
use std::path::PathBuf;
use structopt::clap;

use ya_client_model::activity::ExeScriptCommand;
use ya_core_model::activity;
use ya_service_bus::RpcEnvelope;
use ya_transfer::transfer::TransferService;
use ya_utils_path::normalize_path;

use crate::agreement::Agreement;
use crate::error::Error;
use crate::manifest::ManifestContext;
use crate::message::{GetState, GetStateResponse, Register};
use crate::runtime::process::RuntimeProcess;
use crate::service::metrics::MetricsService;
use crate::service::signal::SignalMonitor;
use crate::state::Supervision;

mod acl;
pub mod agreement;
#[cfg(feature = "sgx")]
pub mod crypto;
pub mod error;
mod handlers;
pub mod logger;
pub mod manifest;
pub mod message;
pub mod metrics;
mod network;
mod notify;
mod output;
pub mod process;
pub mod runtime;
pub mod service;
pub mod state;

mod dns;
mod exe_unit;

pub use exe_unit::{report, ExeUnit, ExeUnitContext, FinishNotifier, RuntimeRef};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(structopt::StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
pub struct Cli {
    /// Runtime binary path
    #[structopt(long, short)]
    pub binary: PathBuf,
    #[structopt(flatten)]
    pub supervise: SuperviseCli,
    /// Additional runtime arguments
    #[structopt(
        long,
        short,
        set = clap::ArgSettings::Global,
        number_of_values = 1,
    )]
    pub runtime_arg: Vec<String>,
    /// Enclave secret key used in secure communication
    #[structopt(
        long,
        env = "EXE_UNIT_SEC_KEY",
        hide_env_values = true,
        set = clap::ArgSettings::Global,
    )]
    #[allow(dead_code)]
    pub sec_key: Option<String>,
    /// Requestor public key used in secure communication
    #[structopt(
        long,
        env = "EXE_UNIT_REQUESTOR_PUB_KEY",
        hide_env_values = true,
        set = clap::ArgSettings::Global,
    )]
    #[allow(dead_code)]
    pub requestor_pub_key: Option<String>,
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(structopt::StructOpt, Debug, Clone)]
pub struct SuperviseCli {
    /// Hardware resources are handled by the runtime
    #[structopt(
        long = "runtime-managed-hardware",
        alias = "cap-handoff",
        parse(from_flag = std::ops::Not::not),
        set = clap::ArgSettings::Global,
    )]
    pub hardware: bool,
    /// Images are handled by the runtime
    #[structopt(
        long = "runtime-managed-image",
        parse(from_flag = std::ops::Not::not),
        set = clap::ArgSettings::Global,
    )]
    pub image: bool,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum Command {
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
    /// Run runtime's test command
    Test,
}

#[derive(structopt::StructOpt, Debug, Clone)]
pub struct RunArgs {
    /// Agreement file path
    #[structopt(long, short)]
    pub agreement: PathBuf,
    /// Working directory
    #[structopt(long, short)]
    pub work_dir: PathBuf,
    /// Common cache directory
    #[structopt(long, short)]
    pub cache_dir: PathBuf,
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
) -> anyhow::Result<crate::crypto::Crypto> {
    let req_key = req_key.ok_or_else(|| anyhow::anyhow!("Missing requestor public key"))?;
    match sec_key {
        Some(key) => Ok(crate::crypto::Crypto::try_with_keys(key, req_key)?),
        None => {
            log::info!("Generating a new key pair...");
            Ok(crate::crypto::Crypto::try_new(req_key)?)
        }
    }
}

pub async fn send_script(
    exe_unit: Addr<ExeUnit<RuntimeProcess>>,
    activity_id: Option<String>,
    exe_script: Vec<ExeScriptCommand>,
) {
    use crate::state::{State, StatePair};
    use std::time::Duration;

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
        batch_id: hex::encode(rand::random::<[u8; 16]>()),
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

// We need this mut for conditional compilation for sgx
#[allow(unused_mut)]
pub async fn run(mut cli: Cli) -> anyhow::Result<()> {
    log::debug!("CLI args: {:?}", cli);

    if !cli.binary.exists() {
        bail!("Runtime binary does not exist: {}", cli.binary.display());
    }

    let mut commands = None;
    let ctx_activity_id;
    let ctx_report_url;

    let args = match cli.command {
        Command::FromFile {
            args,
            service_id,
            report_url,
            input,
        } => {
            let contents = std::fs::read_to_string(&input).map_err(|e| {
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
        Command::Test => {
            let args = cli.runtime_arg.clone();
            let output = ExeUnit::<RuntimeProcess>::test(cli.binary, args)?;
            println!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                bail!("Test failed");
            }
            return Ok(());
        }
    };

    let exe_unit = exe_unit(ExeUnitConfig {
        report_url: ctx_report_url,
        service_id: ctx_activity_id.clone(),
        runtime_args: cli.runtime_arg,
        binary: cli.binary,
        supervise: cli.supervise,
        sec_key: cli.sec_key,
        args,
        requestor_pub_key: cli.requestor_pub_key,
    })
    .await?;

    if let Some(exe_script) = commands {
        tokio::task::spawn(send_script(exe_unit.clone(), ctx_activity_id, exe_script));
    }

    exe_unit.send(FinishNotifier {}).await??.recv().await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ExeUnitConfig {
    pub args: RunArgs,
    pub binary: PathBuf,
    pub runtime_args: Vec<String>,
    pub service_id: Option<String>,
    pub report_url: Option<String>,
    pub supervise: SuperviseCli,

    #[allow(dead_code)]
    pub sec_key: Option<String>,
    #[allow(dead_code)]
    pub requestor_pub_key: Option<String>,
}

pub async fn exe_unit(config: ExeUnitConfig) -> anyhow::Result<Addr<ExeUnit<RuntimeProcess>>> {
    let args = config.args;
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
            hardware: config.supervise.hardware,
            image: config.supervise.image,
            manifest: manifest_ctx,
        },
        activity_id: config.service_id.clone(),
        report_url: config.report_url,
        agreement,
        work_dir,
        cache_dir,
        runtime_args: config.runtime_args,
        acl: Default::default(),
        credentials: None,
        #[cfg(feature = "sgx")]
        crypto: init_crypto(
            config.sec_key.replace("<hidden>".into()),
            config.requestor_pub_key.clone(),
        )?,
    };

    log::debug!("ExeUnitContext args: {:?}", ctx);

    let metrics = MetricsService::try_new(&ctx, Some(10000), ctx.supervise.hardware)?.start();
    let transfers = TransferService::new((&ctx).into()).start();
    let runtime = RuntimeProcess::new(&ctx, config.binary).start();
    let exe_unit = ExeUnit::new(ctx, metrics, transfers, runtime).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();
    exe_unit.send(Register(signals)).await?;

    Ok(exe_unit)
}
