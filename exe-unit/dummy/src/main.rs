mod app;

use anyhow::{Context, Result};
use api::Exec;
use app::{DummyCmd, DummyExeUnit};
use futures::stream::StreamExt;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "dummy")]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: Option<PathBuf>,
}

async fn send_cmds(exe_unit: &mut DummyExeUnit, cmds_json: String) -> Result<()> {
    let mut stream = <DummyExeUnit as Exec<DummyCmd>>::exec(exe_unit, cmds_json);
    while let Some(res) = stream.next().await {
        println!("received response = {:?}", res);
    }
    Ok(())
}

async fn run_interactive() -> Result<()> {
    let mut exe_unit = DummyExeUnit::spawn();
    loop {
        print!("> ");
        io::stdout().flush().context("failed to flush stdout")?;
        let mut cmd = String::new();
        io::stdin()
            .read_line(&mut cmd)
            .context("failed to read line from stdin")?;
        if cmd.is_empty() {
            continue;
        }
        // send to the ExeUnit
        send_cmds(&mut exe_unit, cmd).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    if let Some(input) = opt.input {
        // read JSON
        let cmds_json = fs::read_to_string(&input)
            .with_context(|| format!("failed to read contents of {}", input.display()))?;
        // send to ExeUnit
        let mut exe_unit = DummyExeUnit::spawn();
        send_cmds(&mut exe_unit, cmds_json).await
    } else {
        run_interactive().await
    }
}
