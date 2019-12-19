use actix::prelude::*;
use anyhow::{Context, Result};
use api::{golem::service_bus::BusEntrypoint, prelude::*};
use futures::future;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};
use structopt::StructOpt;
use ya_exe_dummy::worker::{Command, Worker};

#[derive(StructOpt, Debug)]
#[structopt(name = "dummy")]
enum Opt {
    /// Run interactively in CLI mode
    CLI,
    /// Execute commands from JSON file
    FromFile { input: PathBuf },
    /// Bind to the Golem Service Bus
    Gsb { service_id: String },
}

fn main() -> Result<()> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    match opt {
        Opt::CLI => {
            let mut sys = System::new("dummy");
            let worker = Worker::default().start();
            let dispatcher = Dispatcher::new(worker).start();
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
                let _res = sys.block_on(
                    dispatcher
                        .send(CommandFromJson::<Command, _, _>::new(cmd))
                        .then(|stream| match stream {
                            Ok(stream) => future::Either::A(stream.into_inner().for_each(|x| {
                                println!("{:?}", x);
                                future::ok(())
                            })),
                            Err(_) => future::Either::B(future::err(())),
                        }),
                );
            }
        }
        Opt::FromFile { input } => {
            // read JSON
            let cmd = fs::read_to_string(&input)
                .with_context(|| format!("failed to read contents of {}", input.display()))?;
            let mut sys = System::new("dummy");
            let worker = Worker::default().start();
            let dispatcher = Dispatcher::new(worker).start();
            // send to the ExeUnit
            let _res = sys.block_on(
                dispatcher
                    .send(CommandFromJson::<Command, _, _>::new(cmd))
                    .then(|stream| match stream {
                        Ok(stream) => future::Either::A(stream.into_inner().for_each(|x| {
                            println!("{:?}", x);
                            future::ok(())
                        })),
                        Err(_) => future::Either::B(future::err(())),
                    }),
            );
        }
        Opt::Gsb { service_id } => {
            let sys = System::new("dummy");
            let worker = Worker::default().start();
            let dispatcher = Dispatcher::new(worker).start();
            let _bus = BusEntrypoint::<Command, _, _>::new(&service_id, dispatcher).start();
            sys.run()?;
        }
    }

    Ok(())
}
