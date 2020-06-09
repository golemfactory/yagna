use crate::error::Error;
use crate::message::{ExecCmd, ExecCmdResult, SetTaskPackagePath, Shutdown};
use crate::process::ProcessTree;
use crate::runtime::{Runtime, RuntimeArgs};
use crate::ExeUnitContext;
use actix::prelude::*;
use futures::prelude::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use ya_client_model::activity::{CommandResult, ExeScriptCommand};

const PROCESS_KILL_TIMEOUT_SECONDS_ENV_VAR: &str = "PROCESS_KILL_TIMEOUT_SECONDS";
const DEFAULT_PROCESS_KILL_TIMEOUT_SECONDS: i64 = 5;
const MIN_PROCESS_KILL_TIMEOUT_SECONDS: i64 = 1;

fn process_kill_timeout_seconds() -> i64 {
    let limit = std::env::var(PROCESS_KILL_TIMEOUT_SECONDS_ENV_VAR)
        .and_then(|v| v.parse().map_err(|_| std::env::VarError::NotPresent))
        .unwrap_or(DEFAULT_PROCESS_KILL_TIMEOUT_SECONDS);
    std::cmp::max(limit, MIN_PROCESS_KILL_TIMEOUT_SECONDS)
}

pub struct RuntimeProcess {
    binary: PathBuf,
    runtime_args: RuntimeArgs,
    task_package_path: Option<PathBuf>,
    children: HashSet<ProcessTree>,
}

impl RuntimeProcess {
    pub fn new(ctx: &ExeUnitContext, binary: PathBuf) -> Self {
        Self {
            binary,
            runtime_args: ctx.runtime_args.clone(),
            task_package_path: None,
            children: HashSet::new(),
        }
    }

    fn args(&self, cmd_args: Vec<OsString>) -> Result<Vec<OsString>, Error> {
        let pkg_path = self
            .task_package_path
            .clone()
            .ok_or(Error::RuntimeError("Missing task package path".to_owned()))?;

        let mut args = self.runtime_args.to_command_line(&pkg_path);
        args.extend(cmd_args);
        Ok(args)
    }
}

impl Runtime for RuntimeProcess {}

impl Actor for RuntimeProcess {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        log::info!("Runtime handler started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("Runtime handler stopped");
    }
}

#[derive(Debug, Message)]
#[rtype("()")]
struct AddProcessTree(ProcessTree);

#[derive(Debug, Message)]
#[rtype("()")]
struct RemoveProcessTree(ProcessTree);

impl Handler<ExecCmd> for RuntimeProcess {
    type Result = ActorResponse<Self, ExecCmdResult, Error>;

    fn handle(&mut self, msg: ExecCmd, ctx: &mut Self::Context) -> Self::Result {
        let cmd_args = match msg.0.clone() {
            ExeScriptCommand::Deploy {} => {
                let result = vec![OsString::from("deploy")];
                result
            }
            ExeScriptCommand::Start { args } => {
                let mut result = vec![OsString::from("start")];
                result.extend(args.into_iter().map(OsString::from));
                result
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let mut result = vec![
                    OsString::from("run"),
                    OsString::from("--entrypoint"),
                    OsString::from(entry_point),
                ];
                result.extend(args.into_iter().map(OsString::from));
                result
            }
            _ => {
                let fut = futures::future::ok(ExecCmdResult {
                    result: CommandResult::Ok,
                    stdout: None,
                    stderr: None,
                });
                return ActorResponse::r#async(fut.into_actor(self));
            }
        };

        let address = ctx.address();
        let binary = self.binary.clone();
        let args = self.args(cmd_args);
        let current_path = std::env::current_dir();
        log::info!(
            "Executing {:?} with {:?} from path {:?}",
            binary,
            args,
            current_path
        );

        let fut = async move {
            let child = Command::new(binary)
                .kill_on_drop(true)
                .args(args?)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let tree =
                ProcessTree::try_new(child.id()).map_err(|e| Error::RuntimeError(e.to_string()))?;

            address.do_send(AddProcessTree(tree.clone()));
            let result = child.wait_with_output().await;
            address.do_send(RemoveProcessTree(tree));
            let output = result?;

            Ok(ExecCmdResult {
                result: match output.status.success() {
                    true => CommandResult::Ok,
                    _ => CommandResult::Error,
                },
                stdout: Some(vec_to_string(output.stdout)),
                stderr: Some(vec_to_string(output.stderr)),
            })
        };
        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl Handler<SetTaskPackagePath> for RuntimeProcess {
    type Result = <SetTaskPackagePath as Message>::Result;

    fn handle(&mut self, msg: SetTaskPackagePath, _: &mut Self::Context) -> Self::Result {
        self.task_package_path = Some(msg.0);
    }
}

impl Handler<AddProcessTree> for RuntimeProcess {
    type Result = <AddProcessTree as Message>::Result;

    fn handle(&mut self, msg: AddProcessTree, _: &mut Self::Context) -> Self::Result {
        self.children.insert(msg.0);
    }
}

impl Handler<RemoveProcessTree> for RuntimeProcess {
    type Result = <RemoveProcessTree as Message>::Result;

    fn handle(&mut self, msg: RemoveProcessTree, _: &mut Self::Context) -> Self::Result {
        self.children.remove(&msg.0);
    }
}

impl Handler<Shutdown> for RuntimeProcess {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _: Shutdown, _: &mut Self::Context) -> Self::Result {
        let timeout = process_kill_timeout_seconds();
        let futs = self.children.drain().map(move |t| t.kill(timeout));
        futures::future::join_all(futs)
            .map(|_| Ok(()))
            .boxed_local()
    }
}

fn vec_to_string(vec: Vec<u8>) -> String {
    match String::from_utf8(vec) {
        Ok(utf8) => utf8.to_owned(),
        Err(error) => error
            .as_bytes()
            .into_iter()
            .map(|&c| c as char)
            .collect::<String>(),
    }
}
