use anyhow::{Context, Result};
use api::Exec;
use futures::stream::StreamExt;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};
use structopt::StructOpt;
use ya_exe_dummy::{DummyCmd, DummyExeUnit};

#[derive(StructOpt, Debug)]
#[structopt(name = "dummy")]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: Option<PathBuf>,
}

async fn send_cmd(exe_unit: &mut DummyExeUnit, cmd: String) -> Result<()> {
    let mut stream = <DummyExeUnit as Exec<DummyCmd>>::exec(exe_unit, cmd);
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
        let cmd: String = cmd.split_whitespace().collect();
        if cmd.is_empty() {
            println!(
                "you need to specify either a valid JSON command or 'exit' to exit the console"
            );
            continue;
        } else if &cmd == "exit" {
            break;
        }
        // send to the ExeUnit
        send_cmd(&mut exe_unit, cmd).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    if let Some(input) = opt.input {
        // read JSON
        let cmd = fs::read_to_string(&input)
            .with_context(|| format!("failed to read contents of {}", input.display()))?;
        // send to ExeUnit
        let mut exe_unit = DummyExeUnit::spawn();
        send_cmd(&mut exe_unit, cmd).await
    } else {
        run_interactive().await
    }
}
