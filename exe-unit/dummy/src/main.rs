use anyhow::{Context, Result};
use api::Exec;
use futures::{future::FutureExt, pin_mut, select, stream::StreamExt};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};
use structopt::StructOpt;
use tokio::signal;
use ya_exe_dummy::{DummyCmd, DummyExeUnit};

#[derive(StructOpt, Debug)]
#[structopt(name = "dummy")]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: Option<PathBuf>,
}

async fn run_interactive() -> Result<()> {
    let mut exe_unit = DummyExeUnit::spawn();
    let ctrl_c = signal::ctrl_c().fuse();
    pin_mut!(ctrl_c);
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
        // send to the ExeUnit or break on Ctrl-C
        let mut stream = <DummyExeUnit as Exec<DummyCmd>>::exec(&mut exe_unit, cmd).fuse();
        select! {
            _ = ctrl_c => break,
            res = stream.select_next_some() => {
                println!("received response = {:?}", res);
            }
            complete => {},
        }
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
        let mut stream = <DummyExeUnit as Exec<DummyCmd>>::exec(&mut exe_unit, cmds_json);
        while let Some(res) = stream.next().await {
            println!("received response = {:?}", res);
        }
        Ok(())
    } else {
        run_interactive().await
    }
}
