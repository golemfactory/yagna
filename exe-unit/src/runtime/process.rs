use crate::error::Error;
use crate::message::{ExecCmd, ExecCmdResult, Shutdown};
use crate::runtime::Runtime;
use crate::util::Abort;
use crate::ExeUnitContext;
use actix::prelude::*;
use futures::future::{AbortHandle, Abortable};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use tokio::process::Command;
use ya_model::activity::{CommandResult, ExeScriptCommand};

pub struct RuntimeProcess {
    binary: PathBuf,
    image: String,
    work_dir: PathBuf,
    abort_handles: HashSet<Abort>,
}

impl RuntimeProcess {
    pub fn new(ctx: &ExeUnitContext, binary: PathBuf) -> Self {
        Self {
            binary,
            image: ctx.agreement.image.clone(),
            work_dir: ctx.work_dir.clone(),
            abort_handles: HashSet::new(),
        }
    }

    fn args(&self, cmd_args: Vec<OsString>) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("--workdir"),
            self.work_dir.clone().into_os_string(),
            OsString::from("--image"),
            OsString::from(self.image.clone()),
        ];
        args.extend(cmd_args);
        args
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
struct AddChildHandle(Abort);

#[derive(Debug, Message)]
#[rtype("()")]
struct RemoveChildHandle(Abort);

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
                let args = self.args(cmd_args);
                let binary = self.binary.clone();

                log::info!("Executing {:?} with {:?}", binary, args);
                let fut = async move {
                    let child = Command::new(binary)
                        .args(args)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()?;

                    let (handle, reg) = AbortHandle::new_pair();
                    let abort = Abort::from(handle);
                    address.do_send(AddChildHandle(abort.clone()));
                    let result = Abortable::new(child.wait_with_output(), reg).await;
                    address.do_send(RemoveChildHandle(abort));
                    let output =
                        result.map_err(|_| Error::CommandError("aborted".to_owned()))??;

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

impl Handler<AddChildHandle> for RuntimeProcess {
    type Result = <AddChildHandle as Message>::Result;

    fn handle(&mut self, msg: AddChildHandle, _: &mut Self::Context) -> Self::Result {
        self.abort_handles.insert(msg.0);
    }
}

impl Handler<RemoveChildHandle> for RuntimeProcess {
    type Result = <RemoveChildHandle as Message>::Result;

    fn handle(&mut self, msg: RemoveChildHandle, _: &mut Self::Context) -> Self::Result {
        self.abort_handles.remove(&msg.0);
    }
}

impl Handler<Shutdown> for RuntimeProcess {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        for handle in std::mem::replace(&mut self.abort_handles, HashSet::new()).into_iter() {
            handle.abort();
        }
        ctx.stop();
        Ok(())
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
