use actix::prelude::*;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;
use tokio::process::Command;
use ya_core_model::activity::{
    self,
    local::{SetState, SetUsage},
    Exec, GetExecBatchResults,
};
use ya_model::activity::ExeScriptCommand;
use ya_service_bus::{actix_rpc, RpcEnvelope};

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
/// Before running this example you need to provide http server
/// as specified in your `agreement.json` and `commands.json`
/// For default files it is enough to invoke this:
///
///   cargo run --example http-get-put -- -r <path with two files: rust-wasi-tutorial.zip and LICENSE>
#[derive(StructOpt, Debug)]
pub struct Cli {
    /// Supervisor binary
    #[structopt(long, default_value = "target/debug/exe-unit")]
    pub supervisor: PathBuf,
    /// Runtime binary
    #[structopt(long, default_value = "target/debug/wasmtime-exeunit")]
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
    /// timeout in seconds
    #[structopt(long)]
    pub timeout: Option<f32>,
}

struct MockActivityService;

impl Actor for MockActivityService {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        actix_rpc::bind::<SetState>(&ACTIVITY_BUS_ID, addr.clone().recipient());
        actix_rpc::bind::<SetUsage>(&ACTIVITY_BUS_ID, addr.clone().recipient());
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

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("info".into()));
    env_logger::init();

    let args: Cli = Cli::from_args();

    ya_sb_router::bind_gsb_router(None).await?;

    let activity = MockActivityService {};
    activity.start();

    let child_args = vec![
        OsString::from("--binary"),
        OsString::from(&args.runtime),
        OsString::from("-c"),
        OsString::from(&args.cache_dir),
        OsString::from("-w"),
        OsString::from(&args.work_dir),
        OsString::from("-a"),
        OsString::from(&args.agreement),
        OsString::from("service-bus"),
        OsString::from(ACTIVITY_ID),
        OsString::from(ACTIVITY_BUS_ID),
    ];

    let mut child = Command::new(&args.supervisor).args(child_args).spawn()?;
    log::warn!("exeunit supervisor spawned. PID: {}", child.id());
    tokio::time::delay_for(Duration::from_secs(2)).await;

    if let Err(e) = exec_and_wait(&args).await {
        log::error!("executing script {:?} error: {:?}", args.script, e);
    }

    log::warn!("killing exeunit if it is still alive");
    child.kill()?;
    Ok(())
}

async fn exec_and_wait(args: &Cli) -> anyhow::Result<()> {
    let contents = std::fs::read_to_string(&args.script)?;
    let exe_script: Vec<ExeScriptCommand> = serde_json::from_str(&contents)?;
    let exe_len = exe_script.len();
    log::warn!("executing script with {} commands", exe_len);

    let exe_unit_url = activity::exeunit::bus_id(ACTIVITY_ID);
    let exe_unit_service = actix_rpc::service(&exe_unit_url);
    let _ = exe_unit_service
        .send(Exec {
            activity_id: ACTIVITY_ID.to_owned(),
            batch_id: BATCH_ID.to_string(),
            exe_script,
            timeout: None,
        })
        .await?;

    let mut msg = GetExecBatchResults {
        activity_id: ACTIVITY_ID.to_owned(),
        batch_id: BATCH_ID.to_string(),
        timeout: args.timeout,
        command_index: None,
    };

    if args.wait_once {
        log::warn!(
            "waiting at most {:?}s for exe script to complete",
            args.timeout
        );
        let results = exe_unit_service.send(msg).await?;
        log::warn!("Exe script results: {:#?}", results);
        return Ok(());
    }

    for i in 0..exe_len {
        log::warn!("waiting at most {:?}s for {} command", args.timeout, i);

        msg.command_index = Some(i);
        let results = exe_unit_service.send(msg.clone()).await?;

        log::warn!("Command {} result: {:#?}", i, results);
    }
    Ok(())
}
