use actix::prelude::*;
use futures::prelude::*;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;
use tokio::process::Command;
use ya_client_model::activity::{ExeScriptCommand, State};
use ya_core_model::activity::{self, Exec, GetExecBatchResults, GetState, StreamExecBatchResults};
use ya_service_bus::actix_rpc;

const ACTIVITY_BUS_ID: &str = "activity";
const ACTIVITY_ID: &str = "fake_activity_id";
const BATCH_ID: &str = "fake_batch_id";

/// This example allows to test ExeUnit supervisor with GSB.
/// It has two mock services: SetState and SetUsage bound to GSB.
/// ExeUnit should periodically report to those two.
///
/// It tests also Exec and GetExecBatchResults messages support by ExeUnit.
/// Example ends when all ExeScript commands are executed or timeout occurs.
///
///
/// Before running this example you need to provide http server
/// as specified in your `agreement.json` and `commands.json`
/// For default files it is enough to invoke this:
///
///   cargo run -p ya-exe-unit --example http-get-put -- -r <path with two files: rust-wasi-tutorial.zip and LICENSE>
///
/// The timeout parameter can be set to 0 to test out immediate execution result responses.
#[derive(StructOpt, Debug)]
pub struct Cli {
    /// Supervisor binary
    #[structopt(long, default_value = "target/debug/exe-unit")]
    pub supervisor: PathBuf,
    /// Runtime binary
    #[structopt(
        long,
        default_value = "../ya-runtime-wasi/target/debug/ya-runtime-wasi"
    )]
    pub runtime: PathBuf,
    /// Agreement file path (JSON)
    #[structopt(long, short, default_value = "exe-unit/examples/agreement.json")]
    pub agreement: PathBuf,
    /// Working directory
    #[structopt(long, short, default_value = ".")]
    pub work_dir: PathBuf,
    /// Common cache directory
    #[structopt(long, short, default_value = ".")]
    pub cache_dir: PathBuf,
    /// Exe script to run file path (JSON)
    #[structopt(long, default_value = "exe-unit/examples/commands.json")]
    pub script: PathBuf,
    /// Wait strategy. By default this example waits for each consecutive command.
    /// Other strategy is to wait once for the whole script to complete.
    #[structopt(long)]
    pub wait_once: bool,
    /// Execute a scenario where the exe script is interrupted and replaced with another
    #[structopt(long)]
    pub terminate: bool,
    /// timeout in seconds
    #[structopt(long, default_value = "20")]
    pub timeout: f32,
    /// Hand off resource limiting from Supervisor to Runtime
    #[structopt(long)]
    pub cap_handoff: bool,
    /// Stream output during execution
    #[structopt(long)]
    pub stream_output: bool,
}

mod mock_activity {
    use actix::prelude::*;
    use ya_core_model::activity::local::{SetState, SetUsage};
    use ya_service_bus::{actix_rpc, RpcEnvelope};

    pub struct MockActivityService;

    impl Actor for MockActivityService {
        type Context = Context<Self>;

        fn started(&mut self, ctx: &mut Self::Context) {
            let addr = ctx.address();
            actix_rpc::bind::<SetState>(super::ACTIVITY_BUS_ID, addr.clone().recipient());
            actix_rpc::bind::<SetUsage>(super::ACTIVITY_BUS_ID, addr.recipient());
        }
    }

    impl Handler<RpcEnvelope<SetState>> for MockActivityService {
        type Result = <RpcEnvelope<SetState> as Message>::Result;

        fn handle(&mut self, msg: RpcEnvelope<SetState>, _: &mut Self::Context) -> Self::Result {
            log::info!("STATE update: {:?}", msg.into_inner());
            Ok(())
        }
    }

    impl Handler<RpcEnvelope<SetUsage>> for MockActivityService {
        type Result = <RpcEnvelope<SetUsage> as Message>::Result;

        fn handle(&mut self, msg: RpcEnvelope<SetUsage>, _: &mut Self::Context) -> Self::Result {
            log::info!("USAGE update: {:?}", msg.into_inner());
            Ok(())
        }
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("info".into()));
    env_logger::init();

    let args: Cli = Cli::from_args();

    ya_sb_router::bind_gsb_router(None).await?;

    let activity = mock_activity::MockActivityService {};
    activity.start();

    let mut child_args = vec![OsString::from("--binary"), OsString::from(&args.runtime)];
    if args.cap_handoff {
        child_args.insert(0, OsString::from("--cap-handoff"));
    }

    child_args.extend_from_slice(&[
        OsString::from("service-bus"),
        OsString::from(ACTIVITY_ID),
        OsString::from(ACTIVITY_BUS_ID),
        OsString::from("-c"),
        OsString::from(&args.cache_dir),
        OsString::from("-w"),
        OsString::from(&args.work_dir),
        OsString::from("-a"),
        OsString::from(&args.agreement),
    ]);

    let mut child = Command::new(&args.supervisor).args(child_args).spawn()?;
    log::warn!("exeunit supervisor spawned. PID: {:?}", child.id());
    tokio::time::sleep(Duration::from_secs(2)).await;

    if let Err(e) = exec_and_wait(&args).await {
        log::error!("executing script {:?} error: {:?}", args.script, e);
    }

    log::warn!("killing exeunit if it is still alive");
    child.kill().await?;
    Ok(())
}

async fn exec_and_wait(args: &Cli) -> anyhow::Result<()> {
    let contents = std::fs::read_to_string(&args.script)?;
    let mut exe_script: Vec<ExeScriptCommand> = serde_json::from_str(&contents)?;
    log::warn!("executing script with {} command(s)", exe_script.len());

    let exe_unit_url = activity::exeunit::bus_id(ACTIVITY_ID);
    let exe_unit_service = actix_rpc::service(&exe_unit_url);
    let exec = Exec {
        activity_id: ACTIVITY_ID.to_owned(),
        batch_id: BATCH_ID.to_string(),
        exe_script: exe_script.clone(),
        timeout: None,
    };

    let _ = exe_unit_service.send(exec.clone()).await?;

    if args.stream_output {
        let svc = actix_rpc::service(&exe_unit_url);
        tokio::task::spawn_local(async move {
            let msg = StreamExecBatchResults {
                activity_id: ACTIVITY_ID.to_string(),
                batch_id: BATCH_ID.to_string(),
            };
            svc.call_stream(msg)
                .for_each(|r| async move {
                    log::info!("[STREAM] {:?}", r);
                })
                .await;
        });
    }

    let mut msg = GetExecBatchResults {
        activity_id: ACTIVITY_ID.to_string(),
        batch_id: BATCH_ID.to_string(),
        timeout: Some(args.timeout),
        command_index: None,
    };

    if args.wait_once {
        if args.timeout == 0. {
            log::warn!("Immediately requesting full exe script result")
        } else {
            log::warn!(
                "waiting at most {:?}s for exe script to complete",
                args.timeout
            )
        }
        let results = exe_unit_service.send(msg).await?;

        if args.stream_output {
            log::warn!("Exe script results ({})", results?.len());
        } else {
            log::warn!("Exe script results: {:#?}", results);
        }
        return Ok(());
    }

    for i in 0..exe_script.len() {
        msg.command_index = Some(i);
        log::warn!("waiting at most {:?}s for {}. command", msg.timeout, i);
        let result = exe_unit_service.send(msg.clone()).await?;

        if args.stream_output {
            log::warn!("Exe script results ({})", result?.len());
        } else {
            log::warn!("Exe script results: {:#?}", result);
        }

        if args.terminate {
            let response = exe_unit_service
                .send(GetState {
                    activity_id: ACTIVITY_ID.to_string(),
                    timeout: None,
                })
                .await??;
            if response.state.0 == State::Ready {
                break;
            }
        }
    }

    if args.terminate {
        log::warn!("Executing a script starting with TERMINATE");
        let batch_id = "new_batch_id".to_string();

        msg.batch_id = batch_id.clone();
        exe_script.insert(0, ExeScriptCommand::Terminate {});

        let exec = Exec {
            activity_id: ACTIVITY_ID.to_owned(),
            batch_id,
            exe_script: exe_script.clone(),
            timeout: None,
        };

        let _ = exe_unit_service.send(exec.clone()).await?;
        for i in 0..exe_script.len() {
            msg.command_index = Some(i);
            log::warn!("waiting at most {:?}s for {}. command", msg.timeout, i);
            let results = exe_unit_service.send(msg.clone()).await?;

            if args.stream_output {
                log::warn!("Exe script results ({})", results?.len());
            } else {
                log::warn!("Exe script results: {:#?}", results);
            }
        }
    }

    Ok(())
}
