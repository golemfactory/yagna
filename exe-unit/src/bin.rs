use actix::{Actor, System};
use anyhow::bail;
use flexi_logger::{DeferredNow, Record};
use std::convert::TryFrom;
use std::env;
use std::path::PathBuf;
use structopt::{clap, StructOpt};
use ya_core_model::activity;
use ya_exe_unit::agreement::Agreement;
use ya_exe_unit::message::Register;
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::runtime::RuntimeArgs;
use ya_exe_unit::service::metrics::MetricsService;
use ya_exe_unit::service::signal::SignalMonitor;
use ya_exe_unit::service::transfer::TransferService;
use ya_exe_unit::{ExeUnit, ExeUnitContext};
use ya_service_bus::RpcEnvelope;
use ya_utils_path::normalize_path;

#[derive(structopt::StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(version = ya_compile_time_utils::crate_version_commit!())]
pub struct Cli {
    /// Agreement file path
    #[structopt(long, short)]
    pub agreement: PathBuf,
    /// Working directory
    #[structopt(long, short)]
    pub work_dir: PathBuf,
    /// Common cache directory
    #[structopt(long, short)]
    pub cache_dir: PathBuf,
    /// Runtime binary
    #[structopt(long, short)]
    pub binary: PathBuf,
    /// Hand off resource cap limiting to the Runtime
    #[structopt(long = "cap-handoff", parse(from_flag = std::ops::Not::not))]
    pub supervise_caps: bool,
    /// Enclave secret key used in secure communication
    #[structopt(long, env = "EXE_UNIT_SEC_KEY", hide_env_values = true)]
    pub sec_key: Option<String>,
    /// Requestor public key used in secure communication
    #[structopt(long, env = "EXE_UNIT_REQUESTOR_PUB_KEY", hide_env_values = true)]
    pub requestor_pub_key: Option<String>,
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(structopt::StructOpt, Debug)]
pub enum Command {
    /// Execute commands from file
    FromFile { input: PathBuf },
    /// Bind to Service Bus
    ServiceBus {
        service_id: String,
        report_url: String,
    },
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

fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    #[allow(unused_mut)]
    let mut cli: Cli = Cli::from_args();

    if !cli.agreement.exists() {
        bail!("Agreement file does not exist: {}", cli.agreement.display());
    }
    if !cli.binary.exists() {
        bail!("Runtime binary does not exist: {}", cli.binary.display());
    }

    let work_dir = create_path(&cli.work_dir).map_err(|e| {
        anyhow::anyhow!(
            "Cannot create the working directory {}: {}",
            cli.work_dir.display(),
            e
        )
    })?;
    let cache_dir = create_path(&cli.cache_dir).map_err(|e| {
        anyhow::anyhow!(
            "Cannot create the cache directory {}: {}",
            cli.work_dir.display(),
            e
        )
    })?;
    let agreement = Agreement::try_from(&cli.agreement).map_err(|e| {
        anyhow::anyhow!(
            "Error parsing the agreement from {}: {}",
            cli.agreement.display(),
            e
        )
    })?;
    let runtime_args = RuntimeArgs::new(&work_dir, &agreement, !cli.supervise_caps);

    let mut commands = None;
    let mut ctx = ExeUnitContext {
        activity_id: None,
        report_url: None,
        credentials: None,
        agreement,
        work_dir,
        cache_dir,
        runtime_args,
        #[cfg(feature = "sgx")]
        crypto: init_crypto(
            cli.sec_key.replace("<hidden>".into()),
            cli.requestor_pub_key.clone(),
        )?,
    };

    log::debug!("CLI args: {:?}", cli);
    log::debug!("ExeUnitContext args: {:?}", ctx);

    match cli.command {
        Command::FromFile { input } => {
            let contents = std::fs::read_to_string(&input).map_err(|e| {
                anyhow::anyhow!("Cannot read commands from file {}: {}", input.display(), e)
            })?;
            let contents = serde_json::from_str(&contents).map_err(|e| {
                anyhow::anyhow!(
                    "Cannot deserialize commands from file {}: {}",
                    input.display(),
                    e
                )
            })?;
            commands = Some(contents);
        }
        Command::ServiceBus {
            service_id,
            report_url,
        } => {
            ctx.activity_id = Some(service_id);
            ctx.report_url = Some(report_url);
        }
    }

    let sys = System::new("exe-unit");

    let metrics = MetricsService::try_new(&ctx, Some(10000), cli.supervise_caps)?.start();
    let transfers = TransferService::new(&ctx).start();
    let runtime = RuntimeProcess::new(&ctx, cli.binary).start();
    let exe_unit = ExeUnit::new(ctx, metrics, transfers, runtime).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();
    exe_unit.do_send(Register(signals));

    if let Some(exe_script) = commands {
        let msg = activity::Exec {
            activity_id: String::new(),
            batch_id: hex::encode(&rand::random::<[u8; 16]>()),
            exe_script,
            timeout: None,
        };
        exe_unit.do_send(RpcEnvelope::with_caller(String::new(), msg));
    }

    sys.run()?;
    Ok(())
}

pub fn colored_stderr_exeunit_prefixed_format(
    w: &mut dyn std::io::Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(w, "[ExeUnit] ")?;
    flexi_logger::colored_opt_format(w, now, record)
}

fn configure_logger(logger: flexi_logger::Logger) -> flexi_logger::Logger {
    logger
        .format(flexi_logger::colored_opt_format)
        .duplicate_to_stderr(flexi_logger::Duplicate::Debug)
        .format_for_stderr(colored_stderr_exeunit_prefixed_format)
}

fn main() {
    if let Err(_) = configure_logger(flexi_logger::Logger::with_env())
        .log_to_file()
        .directory("logs")
        .start()
    {
        configure_logger(flexi_logger::Logger::with_env())
            .start()
            .expect("Failed to initialize logging");
        log::warn!("Switched to fallback logging method");
    }

    std::process::exit(match run() {
        Ok(_) => 0,
        Err(error) => {
            log::error!("{}", error);
            1
        }
    })
}
