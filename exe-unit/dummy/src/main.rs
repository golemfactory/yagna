mod app;

use self::app::DummyExeUnit;
use anyhow::{anyhow, Context, Result};
use api::core::{HandleCmd, StartCmd, StatusCmd};
use std::{
    convert::TryFrom,
    io::{self, Write},
};

#[derive(Debug)]
enum Cmd {
    Start,
    Status,
    Exit,
}

impl TryFrom<&str> for Cmd {
    type Error = anyhow::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "start" => Ok(Self::Start),
            "status" => Ok(Self::Status),
            "exit" => Ok(Self::Exit),
            s => Err(anyhow!("unknown command: {}", s)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut exe_unit = DummyExeUnit::spawn();

    loop {
        print!("> ");
        io::stdout().flush().context("failed to flush stdout")?;
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .context("failed to read line from stdin")?;
        let args: Vec<&str> = line.split_whitespace().collect();
        if args.is_empty() {
            continue;
        }
        // parse args...
        match Cmd::try_from(args[0]) {
            Err(e) => {
                println!("{}\npossible commands are: start, status, exit", e);
                continue;
            }
            Ok(Cmd::Start) => {
                // start the container...
                // block until computation is finished
                match exe_unit.handle(StartCmd { params: vec![] }).await {
                    Ok(state) => {
                        println!("state changed: {:?}", state);
                        println!("computation finished...")
                    }
                    Err(e) => println!("{}", e),
                }
            }
            Ok(Cmd::Status) => {
                // ask for status...
                let state = exe_unit.handle(StatusCmd).await;
                println!("current state: {:?}", state)
            }
            Ok(Cmd::Exit) => {
                println!("exiting...");
                break;
            }
        }
    }

    Ok(())
}

