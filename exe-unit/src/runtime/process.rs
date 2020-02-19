use crate::error::Error;
use crate::message::{ExecCmd, ExecCmdResult, Shutdown};
use crate::runtime::Runtime;
use crate::ExeUnitContext;
use actix::prelude::*;
use std::path::PathBuf;
use tokio::process::Command;
use ya_model::activity::{CommandResult, ExeScriptCommand};

pub struct RuntimeProcess {
    binary: PathBuf,
    agreement: Option<PathBuf>,
    work_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
}

impl RuntimeProcess {
    pub fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            agreement: None,
            work_dir: None,
            cache_dir: None,
        }
    }

    fn extend_args(&self, mut cmd_args: Vec<String>) -> Vec<String> {
        cmd_args.extend(vec![
            "--workdir".to_owned(),
            format!("{:?}", self.work_dir.as_ref().unwrap()),
            "--cachedir".to_owned(),
            format!("{:?}", self.cache_dir.as_ref().unwrap()),
            "--agreement".to_owned(),
            format!("{:?}", self.agreement.as_ref().unwrap()),
        ]);
        cmd_args
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
        log::debug!("Runtime process started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::debug!("Runtime process stopped");
    }
}

impl Handler<ExecCmd> for RuntimeProcess {
    type Result = ActorResponse<Self, ExecCmdResult, Error>;

    fn handle(&mut self, msg: ExecCmd, _: &mut Self::Context) -> Self::Result {
        let cmd_args = match msg.0 {
            ExeScriptCommand::Transfer { .. } => unimplemented!(),
            ExeScriptCommand::Terminate {} => None,
            ExeScriptCommand::Deploy {} => Some(vec!["deploy".to_owned()]),
            ExeScriptCommand::Start { args } => {
                let mut result = vec!["start".to_owned()];
                result.extend(args);
                Some(result)
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let mut result = vec!["run".to_owned(), entry_point];
                result.extend(args);
                Some(result)
            }
        };

        match cmd_args {
            Some(cmd_args) => {
                log::debug!("Executing {:?}", cmd_args);
                let spawn = Command::new(self.binary.clone())
                    .args(self.extend_args(cmd_args))
                    .spawn();

                let fut = async move {
                    match spawn {
                        Ok(child) => match child.wait_with_output().await {
                            Ok(output) => Ok(ExecCmdResult {
                                result: if output.status.success() {
                                    CommandResult::Ok
                                } else {
                                    CommandResult::Error
                                },
                                message: None,
                                stdout: Some(vec_to_string(output.stdout)),
                                stderr: Some(vec_to_string(output.stderr)),
                            }),
                            Err(error) => Err(Error::from(error)),
                        },
                        Err(error) => Err(Error::from(error)),
                    }
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

impl Handler<Shutdown> for RuntimeProcess {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
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
