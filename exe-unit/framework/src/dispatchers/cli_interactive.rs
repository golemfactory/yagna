use super::dispatcher::Dispatcher;
use super::json_command::*;
pub use crate::supervisor::{
    ExeUnitSupervisor,
    RunCommand,
    StartCommand,
    StopCommand,
    DeployCommand,
    TransferCommand
};

use ya_model::activity::ExeScriptCommand;

use actix::prelude::*;
use anyhow::{Error, Result, Context};
use std::{
    fs,
    io::{self, Write},
};


/// Processes commands from command line in interactive mode.
pub struct InteractiveCli {

}

impl InteractiveCli {
    pub fn new() -> Box<dyn Dispatcher> {
        Box::new(InteractiveCli{})
    }

    fn wait_for_command() -> Result<String> {
        loop {
            print!("> ");
            io::stdout().flush().context("Failed to flush stdout")?;

            let mut cmd = String::new();
            io::stdin()
                .read_line(&mut cmd)
                .context("Failed to read line from stdin")?;

            let cmd: String = cmd.split_whitespace().collect();
            if cmd.is_empty() {
                println!("You need to specify either a valid JSON command or 'exit' to exit the console");
                continue;
            }

            return Ok(cmd);
        }
    }

    fn is_exit_command(cmd: &str) -> bool {
        if cmd == "exit" {
            return true;
        }
        return false;
    }


    fn redirect_to_supervisor(sys: &mut SystemRunner, supervisor: Addr<ExeUnitSupervisor>, command: ExeScriptCommand) -> Result<()> {
        match command {
            ExeScriptCommand::Deploy {} => {
                Ok(sys.block_on(supervisor.send(DeployCommand{}))??)
            },
            ExeScriptCommand::Run {entry_point, args} => {
                Ok(sys.block_on(supervisor.send(RunCommand{entrypoint:entry_point, args}))??)
            },
            ExeScriptCommand::Start {args} => {
                Ok(sys.block_on(supervisor.send(StartCommand{args}))??)
            },
            ExeScriptCommand::Transfer {from, to} => {
                Ok(sys.block_on(supervisor.send(TransferCommand{from, to}))??)
            },
            ExeScriptCommand::Stop {} => {
                Ok(sys.block_on(supervisor.send(StopCommand{}))??)
            }
        }
    }
}


impl Dispatcher for InteractiveCli {

    fn run(&mut self, supervisor: Addr<ExeUnitSupervisor>, mut sys: SystemRunner) -> Result<()> {
        loop {
            let cmd = InteractiveCli::wait_for_command()?;

            if InteractiveCli::is_exit_command(&cmd) {
                break;
            }

            match commands_from_json(&cmd) {
                Ok(commands) => {
                    // send to the ExeUnit
                    for command in commands {
                        InteractiveCli::redirect_to_supervisor(&mut sys, supervisor.clone(), command)?;
                    }
                },
                Err(error) => println!("Invalid command: {}", error)
            }
        }

        Ok(sys.run()?)
    }
}




