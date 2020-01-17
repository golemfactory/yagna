use actix::prelude::*;
use anyhow::{Context, Result};
use api::{golem::service_bus::BusEntrypoint, prelude::*};
use futures::prelude::*;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};
use structopt::StructOpt;
use ya_core_model::activity::{
    Exec, GetActivityState, GetActivityUsage, GetExecBatchResults, GetRunningCommand,
};
use ya_exe_dummy::worker::{Command, Worker};
use ya_service_bus::actix_rpc;

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
                            Ok(stream) => stream
                                .into_inner()
                                .for_each(|x| {
                                    println!("{:?}", x);
                                    future::ready(())
                                })
                                .left_future(),
                            Err(_) => future::ready(()).right_future(),
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
                        Ok(stream) => stream
                            .into_inner()
                            .for_each(|x| {
                                println!("{:?}", x);
                                future::ready(())
                            })
                            .left_future(),
                        Err(_) => future::ready(()).right_future(),
                    }),
            );
        }
        Opt::Gsb { service_id } => {
            let sys = System::new("dummy");
            let worker = Worker::new(&service_id).start();
            let dispatcher = Dispatcher::new(worker.clone()).start();

            BusEntrypoint::<Command, _, _>::new(&service_id, dispatcher.clone()).start();
            actix_rpc::bind::<Exec>(&service_id, worker.clone().recipient());
            actix_rpc::bind::<GetActivityState>(&service_id, worker.clone().recipient());
            actix_rpc::bind::<GetActivityUsage>(&service_id, worker.clone().recipient());
            actix_rpc::bind::<GetRunningCommand>(&service_id, worker.clone().recipient());
            actix_rpc::bind::<GetExecBatchResults>(&service_id, worker.clone().recipient());

            sys.run()?;
        }
    }

    Ok(())
}
