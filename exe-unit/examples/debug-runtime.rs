use actix::Arbiter;
use anyhow::Result;
use futures::channel::mpsc;
use futures::{FutureExt, SinkExt, StreamExt};
use linefeed::{Interface, ReadResult};
use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::str::FromStr;
use structopt::{clap, StructOpt};
use tokio::process::Command;
use tokio_util::codec::{BytesCodec, FramedRead};
use ya_runtime_api::server::{spawn, ProcessStatus, RunProcess, RuntimeEvent, RuntimeService};

/// Deploys and starts a runtime with an interactive prompt{n}
/// debug-runtime --runtime /usr/lib/yagna/plugins/ya-runtime-vm/ya-runtime-vm \{n}
/// --task-package /tmp/image.gvmi \{n}
/// --workdir /tmp/runtime \{n}
/// -- --cpu-cores 4
#[derive(StructOpt)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(rename_all = "kebab-case")]
struct Args {
    /// Runtime binary
    #[structopt(short, long)]
    runtime: PathBuf,
    /// Working directory
    #[structopt(short, long)]
    workdir: PathBuf,
    /// Task package to deploy
    #[structopt(short, long)]
    task_package: PathBuf,
    /// Service protocol version
    #[structopt(short, long, default_value = "0.1.0")]
    version: String,
    /// Skip deployment phase
    #[structopt(
        long = "no-deploy",
        parse(from_flag = std::ops::Not::not),
    )]
    deploy: bool,
    /// Additional runtime arguments
    varargs: Vec<String>,
}

impl Args {
    fn to_args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("--workdir"),
            self.workdir.clone().into_os_string(),
            OsString::from("--task-package"),
            self.task_package.clone().into_os_string(),
        ];
        args.extend(self.varargs.iter().map(OsString::from));
        args
    }
}

struct EventHandler {
    tx: mpsc::Sender<()>,
    arbiter: actix::Arbiter,
}

impl EventHandler {
    pub fn new(tx: mpsc::Sender<()>) -> Self {
        EventHandler {
            tx,
            arbiter: Arbiter::current().clone(),
        }
    }
}

impl RuntimeEvent for EventHandler {
    fn on_process_status(&self, status: ProcessStatus) {
        if !status.stdout.is_empty() {
            let out = String::from_utf8_lossy(status.stdout.as_slice());
            let mut stdout = std::io::stdout();
            stdout.write_all(out.as_bytes()).unwrap();
            stdout.flush().unwrap();
        }
        if !status.stderr.is_empty() {
            let out = String::from_utf8_lossy(status.stderr.as_slice());
            let mut stderr = std::io::stderr();
            stderr.write_all(out.as_bytes()).unwrap();
            stderr.flush().unwrap();
        }
        if !status.running {
            let mut tx = self.tx.clone();
            self.arbiter.send(
                async move {
                    let _ = tx.send(()).await;
                }
                .boxed(),
            );
        }
    }
}

fn forward_output<F, R>(read: R, mut f: F)
where
    F: FnMut(Vec<u8>) -> () + 'static,
    R: tokio::io::AsyncRead + 'static,
{
    let stream = FramedRead::new(read, BytesCodec::new())
        .filter_map(|result| async { result.ok() })
        .ready_chunks(16)
        .map(|v| v.into_iter().map(|b| b.to_vec()).flatten().collect());
    Arbiter::spawn(stream.for_each(move |e| futures::future::ready(f(e))));
}

async fn deploy(args: &Args) -> Result<()> {
    let mut rt_args = args.to_args();
    rt_args.push(OsString::from("deploy"));

    log::info!("deploying {} {:?}", args.runtime.display(), rt_args);

    let mut child = Command::new(&args.runtime)
        .kill_on_drop(true)
        .args(rt_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdout = std::io::stdout();
    forward_output(child.stdout.take().unwrap(), move |out| {
        let cow = String::from_utf8_lossy(out.as_slice());
        let out = cow.trim();
        if !out.is_empty() {
            stdout.write_all(out.as_bytes()).unwrap();
            stdout.flush().unwrap();
        }
    });

    let mut stderr = std::io::stderr();
    forward_output(child.stderr.take().unwrap(), move |out| {
        let cow = String::from_utf8_lossy(out.as_slice());
        let out = cow.trim();
        if !out.is_empty() {
            stderr.write_all(out.as_bytes()).unwrap();
            stderr.flush().unwrap();
        }
    });

    child.await?;
    Ok(())
}

async fn start(args: Args) -> Result<()> {
    let mut rt_args = args.to_args();
    rt_args.push(OsString::from("start"));

    log::info!("starting {} {:?}", args.runtime.display(), rt_args);
    let mut command = Command::new(&args.runtime);
    command.args(rt_args);

    let (tx, mut rx) = mpsc::channel(1);
    let service = spawn(command, EventHandler::new(tx)).await?;

    log::info!("sending hello");
    let _ = service.hello(args.version.as_str()).await;

    log::info!("press Ctrl+D to exit");
    loop {
        let interface = Interface::new("run")?;
        interface.set_prompt("$ ")?;

        let input = match interface.read_line()? {
            ReadResult::Eof | ReadResult::Signal(_) => break,
            ReadResult::Input(input) => match input.is_empty() {
                true => continue,
                _ => input,
            },
        };

        if let Err(e) = run(service.clone(), input).await {
            log::error!("command error: {}", e);
        } else {
            let _ = rx.next().await;
        }
    }

    log::info!("shutting down...");
    if let Err(e) = service.shutdown().await {
        log::error!("shutdown error: {:?}", e);
    }
    Ok(())
}

async fn run(service: impl RuntimeService, input: String) -> Result<()> {
    let mut args = shell_words::split(input.as_str())?;
    if args.len() == 0 {
        return Ok(());
    }

    let bin_path = PathBuf::from_str(args.remove(0).as_str())?;
    let bin_name = bin_path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid command: {}", bin_path.display()))?
        .to_string_lossy()
        .to_string();

    let mut run_process = RunProcess::default();
    run_process.bin = bin_path.display().to_string();
    run_process.args = std::iter::once(bin_name)
        .chain(args.iter().map(|s| s.clone()))
        .collect();

    log::info!("running {} {:?}", run_process.bin, run_process.args);
    service
        .run_process(run_process)
        .await
        .map_err(|e| anyhow::anyhow!("process error: {:?}", e))?;

    Ok(())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();

    let mut args = Args::from_args();
    args.runtime = args.runtime.canonicalize()?;

    if args.deploy {
        deploy(&args).await?;
    }
    start(args).await?;

    Ok(())
}
