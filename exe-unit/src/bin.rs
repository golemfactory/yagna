use std::convert::TryFrom;
use std::path::PathBuf;

use actix::{Actor, Addr, Arbiter, System};
use anyhow::bail;
use flexi_logger::{DeferredNow, Record};
use structopt::{clap, StructOpt};
use ya_client_model::activity::ExeScriptCommand;
use ya_service_bus::RpcEnvelope;

use ya_core_model::activity;
use ya_exe_unit::agreement::Agreement;
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
    /// Enclave secret key used in secure communication
    #[structopt(
    long,
    env = "EXE_UNIT_SEC_KEY",
    hide_env_values = true,
    set = clap::ArgSettings::Global,
    )]
    sec_key: Option<String>,
    /// Requestor public key used in secure communication
    #[structopt(
    long,
    env = "EXE_UNIT_REQUESTOR_PUB_KEY",
    hide_env_values = true,
    set = clap::ArgSettings::Global,
    )]
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
            _ => tokio::time::delay_for(delay).await,
        }
    }

    log::debug!("Executing commands: {:?}", exe_script);

    let msg = activity::Exec {
        activity_id: activity_id.unwrap_or_else(Default::default),
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

fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
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
                anyhow::anyhow!("Cannot read commands from file {}: {}", input.display(), e)
            })?;
            let contents = serde_json::from_str(&contents).map_err(|e| {
                anyhow::anyhow!(
                    "Cannot deserialize commands from file {}: {}",
                    input.display(),
                    e
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
            let offer_template = ExeUnit::<RuntimeProcess>::offer_template(cli.binary)?;
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
            "Cannot create the working directory {}: {}",
            args.work_dir.display(),
            e
        )
    })?;
    let cache_dir = create_path(&args.cache_dir).map_err(|e| {
        anyhow::anyhow!(
            "Cannot create the cache directory {}: {}",
            args.work_dir.display(),
            e
        )
    })?;
    let agreement = Agreement::try_from(&args.agreement).map_err(|e| {
        anyhow::anyhow!(
            "Error parsing the agreement from {}: {}",
            args.agreement.display(),
            e
        )
    })?;

    let ctx = ExeUnitContext {
        supervise: Supervision {
            hardware: cli.supervise.hardware,
            image: cli.supervise.image,
        },
        activity_id: ctx_activity_id.clone(),
        report_url: ctx_report_url,
        agreement,
        work_dir,
        cache_dir,
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

    let sys = System::new("exe-unit");

    let metrics = MetricsService::try_new(&ctx, Some(10000), ctx.supervise.hardware)?.start();
    let transfers = TransferService::new(&ctx).start();
    let runtime = RuntimeProcess::new(&ctx, cli.binary).start();
    let exe_unit = ExeUnit::new(ctx, metrics, transfers, runtime).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();
    exe_unit.do_send(Register(signals));

    if let Some(exe_script) = commands {
        Arbiter::spawn(send_script(exe_unit, ctx_activity_id, exe_script));
    }

    sys.run()?;
    Ok(())
}

pub fn colored_stderr_exeunit_prefixed_format(
    w: &mut dyn std::io::Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(w, "{}", yansi::Color::Fixed(92).paint("[ExeUnit] "))?;
    flexi_logger::colored_opt_format(w, now, record)
}

fn configure_logger(logger: flexi_logger::Logger) -> flexi_logger::Logger {
    logger
        .format(flexi_logger::colored_opt_format)
        .duplicate_to_stderr(flexi_logger::Duplicate::Debug)
        .format_for_stderr(colored_stderr_exeunit_prefixed_format)
}

fn main() {
    let default_log_level = "info";
    if configure_logger(flexi_logger::Logger::with_env_or_str(default_log_level))
        .log_to_file()
        .directory("logs")
        .start()
        .is_err()
    {
        configure_logger(flexi_logger::Logger::with_env_or_str(default_log_level))
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

#[cfg(test)]
mod test {
    #[test]
    fn test_paint() {
        println!("Some: {}", yansi::Color::Fixed(92).paint("violet text!"));
    }
}
