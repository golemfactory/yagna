use crate::error::Error;
use crate::message::{ExecCmd, ExecCmdResult, Shutdown};
use crate::runtime::Runtime;
use crate::ExeUnitContext;
use actix::prelude::*;
use futures::future::{AbortHandle, Abortable};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use tokio::process::Command;
use ya_model::activity::{CommandResult, ExeScriptCommand};

pub struct RuntimeProcess {
    binary: PathBuf,
    agreement: Option<PathBuf>,
    work_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    child_handle: Option<AbortHandle>,
}

impl RuntimeProcess {
    pub fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            agreement: None,
            work_dir: None,
            cache_dir: None,
            child_handle: None,
        }
    }

    fn args(&self, cmd_args: Vec<OsString>) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("--agreement"),
            self.agreement.clone().unwrap().into_os_string(),
            OsString::from("--cachedir"),
            self.cache_dir.clone().unwrap().into_os_string(),
            OsString::from("--workdir"),
            self.work_dir.clone().unwrap().into_os_string(),
        ];
        args.extend(cmd_args);
        args
    }
}

impl Runtime for RuntimeProcess {
    fn with_context(mut self, ctx: ExeUnitContext) -> Self {
        self.agreement = Some(ctx.agreement);
        self.work_dir = Some(ctx.work_dir);
        self.cache_dir = Some(ctx.cache_dir);
        self
    }
}

impl Actor for RuntimeProcess {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        log::debug!("Runtime handler started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::debug!("Runtime handler stopped");
    }
}

trait MapSelf<T, E>
where
    Self: Sized,
{
    fn map_self<F: FnOnce(Self) -> Result<T, E>>(self, f: F) -> Result<T, E>;
}

impl<T, E, Tm, Em> MapSelf<Tm, Em> for Result<T, E>
where
    Self: Sized,
{
    fn map_self<F: FnOnce(Self) -> Result<Tm, Em>>(self, f: F) -> Result<Tm, Em> {
        f(self)
    }
}

#[derive(Debug, Message)]
#[rtype("()")]
struct ClearChildHandle;

impl Handler<ExecCmd> for RuntimeProcess {
    type Result = ActorResponse<Self, ExecCmdResult, Error>;

    fn handle(&mut self, msg: ExecCmd, ctx: &mut Self::Context) -> Self::Result {
        let cmd_args = match msg.0.clone() {
            ExeScriptCommand::Transfer { .. } => None,
            ExeScriptCommand::Terminate {} => None,
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
        };

        let address = ctx.address();
        match cmd_args {
            Some(cmd_args) => {
                let args = self.args(cmd_args);

                log::debug!("Executing {:?}", args);
                let spawn = Command::new(self.binary.clone())
                    .args(args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn();

                let (handle, reg) = AbortHandle::new_pair();
                self.child_handle = Some(handle);

                let fut = async move {
                    let output = Abortable::new(spawn?.wait_with_output(), reg)
                        .await
                        .map_self(|r| {
                            address.do_send(ClearChildHandle {});
                            r.map_err(|_| Error::CommandError("Process aborted".to_owned()))
                        })?
                        .map_self(|r| {
                            address.do_send(ClearChildHandle {});
                            r
                        })?;

                    Ok(ExecCmdResult {
                        result: output_to_result(&output),
                        message: None,
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
                        message: None,
                        stdout: None,
                        stderr: None,
                    })
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
        }
    }
}

impl Handler<ClearChildHandle> for RuntimeProcess {
    type Result = <ClearChildHandle as Message>::Result;

    fn handle(&mut self, _: ClearChildHandle, _: &mut Self::Context) -> Self::Result {
        self.child_handle = None;
    }
}

impl Handler<Shutdown> for RuntimeProcess {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        if let Some(handle) = &self.child_handle {
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
