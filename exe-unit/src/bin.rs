use actix::{Actor, System};
use structopt::StructOpt;
use ya_core_model::activity::Exec;
use ya_exe_unit::cli::{Cli, Command};
use ya_exe_unit::message::Register;
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::service::signal::SignalMonitor;
use ya_exe_unit::{ExeUnit, ExeUnitContext};
use ya_service_bus::RpcEnvelope;

// Temporary
const BINARY: &'static str = "wasmtime";

fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli: Cli = Cli::from_args();
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

    let activity_id = match &ctx.service_id {
        Some(service_id) => service_id.clone(),
        None => "".to_owned(),
    };
    let runtime = RuntimeProcess::new(BINARY.into());
    let exe_unit = ExeUnit::new(ctx, runtime).start();
    let signals = SignalMonitor::new(exe_unit.clone()).start();

    exe_unit.do_send(Register(signals));

    if let Some(exe_script) = commands {
        let msg = Exec {
            activity_id: activity_id.clone(),
            batch_id: "cli-batch".to_owned(),
            exe_script,
            timeout: None,
        };
        exe_unit.do_send(RpcEnvelope::with_caller(activity_id, msg));
    }

    sys.run()?;
    Ok(())
}
