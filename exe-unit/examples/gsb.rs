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
const ACTIVITY_ID: &str = "0x07";

#[derive(StructOpt, Debug)]
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
    /// Supervisor binary
    #[structopt(long)]
    pub supervisor: PathBuf,
    /// Runtime binary
    #[structopt(long)]
    pub runtime: PathBuf,
    #[structopt(long)]
    pub script: PathBuf,
}

struct Activity;

impl Actor for Activity {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        actix_rpc::bind::<SetState>(&ACTIVITY_BUS_ID, addr.clone().recipient());
        actix_rpc::bind::<SetUsage>(&ACTIVITY_BUS_ID, addr.clone().recipient());
    }
}

impl Handler<RpcEnvelope<SetState>> for Activity {
    type Result = <RpcEnvelope<SetState> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<SetState>, _: &mut Self::Context) -> Self::Result {
        log::info!("STATE update: {:?}", msg.into_inner());
        Ok(())
    }
}

impl Handler<RpcEnvelope<SetUsage>> for Activity {
    type Result = <RpcEnvelope<SetUsage> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<SetUsage>, _: &mut Self::Context) -> Self::Result {
        log::info!("USAGE update: {:?}", msg.into_inner());
        Ok(())
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    println!(
        r#"

    >> YO! YAGNA DEV <<

    Before running this example you should also start:
      cargo run --example http-get-put -- -r <path with two files: rust-wasi-tutorial.zip and LICENSE>
    and
      cargo run --example ya_sb_router -- -l 127.0.0.1:7464
    "#
    );
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("info".into()));
    env_logger::init();

    let args: Cli = Cli::from_args();

    let activity = Activity {};
    activity.start();

    let child_args = vec![
        OsString::from("--binary"),
        OsString::from(args.runtime),
        OsString::from("-c"),
        OsString::from(args.cache_dir),
        OsString::from("-w"),
        OsString::from(args.work_dir),
        OsString::from("-a"),
        OsString::from(args.agreement),
        OsString::from("service-bus"),
        OsString::from(ACTIVITY_ID),
        OsString::from(ACTIVITY_BUS_ID),
    ];

    let contents = std::fs::read_to_string(&args.script)?;
    let exe_script: Vec<ExeScriptCommand> = serde_json::from_str(&contents)?;
    let exe_len = exe_script.len();
    let mut child = Command::new(args.supervisor).args(child_args).spawn()?;
    tokio::time::delay_for(Duration::from_secs(2)).await;

    let batch_id = "fake_batch_id";
    let exe_unit_url = activity::exeunit::bus_id(ACTIVITY_ID);
    let _ = actix_rpc::service(&exe_unit_url)
        .send(Exec {
            activity_id: ACTIVITY_ID.to_owned(),
            batch_id: batch_id.to_string(),
            exe_script,
            timeout: None,
        })
        .await?;

    for i in 0..exe_len {
        let results = actix_rpc::service(&exe_unit_url)
            .send(GetExecBatchResults {
                activity_id: ACTIVITY_ID.to_owned(),
                batch_id: batch_id.to_string(),
                timeout: Some(10.0),
                command_index: Some(i),
            })
            .await?;

        log::info!("Command result {}: {:?}", i, results);
    }

    child.kill()?;
    Ok(())
}
