use actix::{Actor, System};
use std::path::PathBuf;
use structopt::StructOpt;
use ya_core_model::activity::Exec;
use ya_exe_unit::message::Register;
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::service::signal::SignalMonitor;
use ya_exe_unit::{ExeUnit, ExeUnitContext};
use ya_service_bus::RpcEnvelope;

// Temporary
const BINARY: &'static str = "wasmtime";

#[derive(structopt::StructOpt, Debug)]
pub struct Cli {
    #[structopt(long, short)]
    pub agreement: PathBuf,
    #[structopt(long, short)]
    pub work_dir: PathBuf,
    #[structopt(long, short)]
    pub cache_dir: PathBuf,
    #[structopt(long, short)]
    pub binary: Option<PathBuf>,
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(structopt::StructOpt, Debug)]
pub enum Command {
    ServiceBus {
        service_id: String,
        report_url: String,
    },
    FromFile {
        input: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let cli: Cli = Cli::from_args();
    let binary = cli.binary.clone().unwrap_or(PathBuf::from(BINARY));
    let mut commands = None;
    let mut ctx = ExeUnitContext {
        service_id: None,
        report_url: None,
        agreement: cli.agreement,
        work_dir: cli.work_dir,
        cache_dir: cli.cache_dir,
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
            ctx.service_id = Some(service_id);
            ctx.report_url = Some(report_url);
        }
    }

    let sys = System::new("exe-unit");
    let exe_unit = ExeUnit::new(ctx, RuntimeProcess::new(binary)).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();
    exe_unit.do_send(Register(signals));

    if let Some(exe_script) = commands {
        let msg = Exec {
            activity_id: String::new(),
            batch_id: String::new(),
            exe_script,
            timeout: None,
        };
        exe_unit.do_send(RpcEnvelope::with_caller(String::new(), msg));
    }

    sys.run()?;
    Ok(())
}
