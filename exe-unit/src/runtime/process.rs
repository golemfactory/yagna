use crate::error::Error;
use crate::message::{ExecCmd, ExecCmdResult, SetTaskPackagePath, Shutdown};
use crate::runtime::Runtime;
use crate::ExeUnitContext;
use actix::prelude::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use tokio::process::Command;
use ya_model::activity::{CommandResult, ExeScriptCommand};

const PROCESS_KILL_TIMEOUT_SEC: u64 = 5;

pub struct RuntimeProcess {
    binary: PathBuf,
    work_dir: PathBuf,
    task_package_path: Option<PathBuf>,
    children: HashSet<u32>,
}

impl RuntimeProcess {
    pub fn new(ctx: &ExeUnitContext, binary: PathBuf) -> Self {
        Self {
            binary,
            work_dir: ctx.work_dir.clone(),
            task_package_path: None,
            children: HashSet::new(),
        }
    }

    fn args(&self, cmd_args: Vec<OsString>) -> Result<Vec<OsString>, Error> {
        let pkg_path = self
            .task_package_path
            .clone()
            .ok_or(Error::RuntimeError("Task package path missing".to_owned()))?;
        let mut args = vec![
            OsString::from("--workdir"),
            self.work_dir.clone().into_os_string(),
            OsString::from("--task-package"),
            OsString::from(pkg_path),
        ];
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
struct AddChild(u32);

#[derive(Debug, Message)]
#[rtype("()")]
struct RemoveChild(u32);

impl Handler<ExecCmd> for RuntimeProcess {
    type Result = ActorResponse<Self, ExecCmdResult, Error>;

    fn handle(&mut self, msg: ExecCmd, ctx: &mut Self::Context) -> Self::Result {
        let cmd_args = match msg.0.clone() {
            ExeScriptCommand::Deploy {} => Some(vec![OsString::from("deploy")]),
            ExeScriptCommand::Start { args } => {
                let mut result = vec![OsString::from("start")];
                result.extend(args.into_iter().map(OsString::from));
                Some(result)
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let mut result = vec![
                    OsString::from("run"),
                    OsString::from("--entrypoint"),
                    OsString::from(entry_point),
                ];
                result.extend(args.into_iter().map(OsString::from));
                Some(result)
            }
            _ => None,
        };

        let address = ctx.address();
        match cmd_args {
            Some(cmd_args) => {
                let binary = self.binary.clone();
                let args = self.args(cmd_args);
                log::info!("Executing {:?} with {:?}", binary, args);

                let fut = async move {
                    let child = Command::new(binary)
                        .kill_on_drop(true)
                        .args(args?)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()?;
                    let pid = child.id();

                    address.do_send(AddChild(pid));
                    let result = child.wait_with_output().await;
                    address.do_send(RemoveChild(pid));
                    let output = result?;

                    Ok(ExecCmdResult {
                        result: output_to_result(&output),
                        stdout: Some(vec_to_string(output.stdout)),
                        stderr: Some(vec_to_string(output.stderr)),
                    })
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            None => {
                let fut = async {
                    Ok(ExecCmdResult {
                        result: CommandResult::Ok,
                        stdout: None,
                        stderr: None,
                    })
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
        }
    }
}

impl Handler<SetTaskPackagePath> for RuntimeProcess {
    type Result = <SetTaskPackagePath as Message>::Result;

    fn handle(&mut self, msg: SetTaskPackagePath, _: &mut Self::Context) -> Self::Result {
        self.task_package_path = Some(msg.0);
    }
}

impl Handler<AddChild> for RuntimeProcess {
    type Result = <AddChild as Message>::Result;

    fn handle(&mut self, msg: AddChild, _: &mut Self::Context) -> Self::Result {
        self.children.insert(msg.0);
    }
}

impl Handler<RemoveChild> for RuntimeProcess {
    type Result = <RemoveChild as Message>::Result;

    fn handle(&mut self, msg: RemoveChild, _: &mut Self::Context) -> Self::Result {
        self.children.remove(&msg.0);
    }
}

impl Handler<Shutdown> for RuntimeProcess {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _: Shutdown, _: &mut Self::Context) -> Self::Result {
        let children = std::mem::replace(&mut self.children, HashSet::new());
        let fut = async move {
            futures::future::join_all(children.into_iter().map(kill_pid)).await;
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

#[inline]
fn output_to_result(output: &Output) -> CommandResult {
    if output.status.success() {
        CommandResult::Ok
    } else {
        CommandResult::Error
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

#[cfg(windows)]
async fn kill_pid(pid: u32) {
    // FIXME: implement for win32
    unimplemented!()
}

#[cfg(not(windows))]
async fn kill_pid(pid: u32) {
    use chrono::Local;
    use nix::sys::signal;
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    use std::time::Duration;

    fn alive(pid: Pid) -> bool {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status) => match status {
                WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _) => false,
                _ => true,
            },
            _ => false,
        }
    }

    let pid = Pid::from_raw(pid as i32);
    let delay = Duration::from_secs_f32(PROCESS_KILL_TIMEOUT_SEC as f32 / 10.);
    let started = Local::now().timestamp();

    if let Ok(_) = signal::kill(pid, signal::Signal::SIGTERM) {
        log::info!("Sent SIGTERM to process {:?}", pid);

        loop {
            let elapsed = Local::now().timestamp() >= started + PROCESS_KILL_TIMEOUT_SEC as i64;

            match alive(pid) {
                true => {
                    if elapsed {
                        log::info!("Sending SIGKILL to process {:?}", pid);
                        let _ = signal::kill(pid, signal::Signal::SIGKILL);
                    }
                }
                _ => break,
            }

            match elapsed {
                true => break,
                _ => tokio::time::delay_for(delay).await,
            }
        }
    }
}
