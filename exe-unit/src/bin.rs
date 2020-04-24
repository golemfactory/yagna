use actix::{Actor, System};
use anyhow::bail;
use flexi_logger::{DeferredNow, Record};
use std::convert::TryFrom;
use std::path::PathBuf;
use structopt::StructOpt;
use ya_core_model::activity;
use ya_exe_unit::agreement::Agreement;
use ya_exe_unit::message::Register;
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::service::metrics::MetricsService;
use ya_exe_unit::service::signal::SignalMonitor;
use ya_exe_unit::service::transfer::TransferService;
use ya_exe_unit::{ExeUnit, ExeUnitContext};
use ya_service_bus::RpcEnvelope;

#[derive(structopt::StructOpt, Debug)]
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
    Ok(path.canonicalize()?)
}

fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli: Cli = Cli::from_args();
    let mut commands = None;
    let mut ctx = ExeUnitContext {
        activity_id: None,
        report_url: None,
        agreement: Agreement::try_from(&cli.agreement)?,
        work_dir: create_path(&cli.work_dir)?,
        cache_dir: create_path(&cli.cache_dir)?,
    };

    match cli.command {
        Command::FromFile { input } => {
            let contents = std::fs::read_to_string(&input)?;
            commands = Some(serde_json::from_str(&contents)?);
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

    let metrics = MetricsService::try_new(&ctx, Some(10000))?.start();
    let transfers = TransferService::new(&ctx).start();
    let runtime = RuntimeProcess::new(&ctx, cli.binary).start();
    let exe_unit = ExeUnit::new(ctx, metrics, transfers, runtime).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();
    exe_unit.do_send(Register(signals));

    if let Some(exe_script) = commands {
        let msg = activity::Exec {
            activity_id: String::new(),
            batch_id: "fake_batch_id".into(),
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

fn main() -> anyhow::Result<()> {
    flexi_logger::Logger::with_env()
        .log_to_file()
        .directory("logs")
        .format(flexi_logger::colored_opt_format)
        .duplicate_to_stderr(flexi_logger::Duplicate::Debug)
        .format_for_stderr(colored_stderr_exeunit_prefixed_format)
        .start()?;

    let result = run();
    if let Err(error) = result {
        log::error!("Exiting with error: {}", error);
        return Err(error);
    }

    Ok(result?)
}
