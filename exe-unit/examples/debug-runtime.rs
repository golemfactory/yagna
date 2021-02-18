use actix::{Arbiter, System};
use anyhow::{Context, Result};
use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, SinkExt, StreamExt};
use linefeed::{Interface, ReadResult, Signal, Terminal};
use std::ffi::OsString;
use std::fs::{create_dir_all, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use structopt::{clap, StructOpt};
use tokio::process::Command;
use tokio_util::codec::{BytesCodec, FramedRead};
use ya_runtime_api::server::{spawn, ProcessStatus, RunProcess, RuntimeEvent, RuntimeService};
use ya_utils_path::data_dir::DataDir;

lazy_static::lazy_static! {
    static ref COLOR_MISC: ansi_term::Style = ansi_term::Color::Purple.bold();
    static ref COLOR_INFO: ansi_term::Style = ansi_term::Color::White.bold();
    static ref COLOR_ERR: ansi_term::Style = ansi_term::Color::Red.bold();
    static ref COLOR_PROMPT: ansi_term::Style = ansi_term::Color::Purple.bold();
}

macro_rules! misc {
    ($dst:expr, $($arg:tt)*) => (
        writeln!(
            $dst,
            "{}{} {}",
            (*COLOR_MISC).prefix(),
            format!($($arg)*),
            (*COLOR_MISC).suffix()
        ).expect("unable to write to stdout")
    );
}

macro_rules! info {
    ($dst:expr, $($arg:tt)*) => (
        writeln!(
            $dst,
            "{}[INFO] {} {}",
            (*COLOR_INFO).prefix(),
            format!($($arg)*),
            (*COLOR_INFO).suffix()
        ).expect("unable to write to stdout")
    );
}

macro_rules! err {
    ($dst:expr, $($arg:tt)*) => (
        writeln!(
            $dst,
            "{}[ERR ] {} {}",
            (*COLOR_ERR).prefix(),
            format!($($arg)*),
            (*COLOR_ERR).suffix()
        ).expect("unable to write to stdout")
    );
}
/// Deploys and starts a runtime with an interactive prompt{n}
/// debug-runtime --runtime /usr/lib/yagna/plugins/ya-runtime-vm/ya-runtime-vm \{n}
/// --task-package /tmp/image.gvmi \{n}
/// --workdir /tmp/runtime \{n}
/// -- --cpu-cores 4
#[derive(StructOpt)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
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
    fn to_runtime_args(&self) -> Vec<OsString> {
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

struct EventHandler<T: Terminal> {
    tx: mpsc::Sender<()>,
    arbiter: actix::Arbiter,
    ui: UI<T>,
}

impl<T: Terminal> EventHandler<T> {
    pub fn new(tx: mpsc::Sender<()>, ui: UI<T>) -> Self {
        let arbiter = Arbiter::current().clone();
        EventHandler { tx, ui, arbiter }
    }
}

impl<T: Terminal + 'static> RuntimeEvent for EventHandler<T> {
    fn on_process_status(&self, status: ProcessStatus) {
        if !status.stdout.is_empty() {
            write_output(&self.ui, status.stdout);
        }
        if !status.stderr.is_empty() {
            write_output(&self.ui, status.stderr);
        }
        if !status.running {
            match status.return_code {
                0 => (),
                c => err!(self.ui, "command failed with code {}", c),
            }

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

fn forward_output<R, T>(read: R, mut writer: UI<T>)
where
    R: tokio::io::AsyncRead + 'static,
    T: Terminal + 'static,
{
    let stream = FramedRead::new(read, BytesCodec::new())
        .filter_map(|result| async { result.ok() })
        .ready_chunks(16)
        .map(|v| v.into_iter().map(|b| b.to_vec()).flatten().collect());
    Arbiter::spawn(async move {
        stream
            .for_each(move |v| futures::future::ready(write_output(&mut writer, v)))
            .await;
    });
}

fn write_output<T>(writer: &UI<T>, out: Vec<u8>)
where
    T: Terminal + 'static,
{
    let cow = String::from_utf8_lossy(out.as_slice());
    let out = cow.trim();
    if !out.is_empty() {
        write!(writer, "{}", out).unwrap();
    }
}

async fn deploy<T>(args: &Args, ui: UI<T>) -> Result<()>
where
    T: Terminal + 'static,
{
    let mut rt_args = args.to_runtime_args();
    rt_args.push(OsString::from("deploy"));

    info!(ui, "Deploying");

    let _ = create_dir_all(&args.workdir);
    let mut child = runtime_command(&args)?
        .kill_on_drop(true)
        .args(rt_args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    // forward_output(child.stdout.take().unwrap(), ui.clone());
    forward_output(child.stderr.take().unwrap(), ui.clone());

    if !child.await?.success() {
        return Err(anyhow::anyhow!("deployment failed"));
    }

    Ok(())
}

async fn start<T>(
    args: Args,
    mut input_rx: mpsc::Receiver<String>,
    start_tx: oneshot::Sender<()>,
    ui: UI<T>,
) -> Result<()>
where
    T: Terminal + 'static,
{
    let mut rt_args = args.to_runtime_args();
    rt_args.push(OsString::from("start"));

    info!(ui, "Starting");

    let mut command = runtime_command(&args)?;
    command.args(rt_args);

    let (tx, mut rx) = mpsc::channel(1);
    let service = spawn(command, EventHandler::new(tx, ui.clone()))
        .await
        .context("unable to spawn runtime")?;
    let _ = service.hello(args.version.as_str()).await;

    info!(ui, "Entering prompt, press C-d to exit");
    let _ = start_tx.send(());

    while let Some(input) = input_rx.next().await {
        if let Err(e) = run(service.clone(), input).await {
            let message = e.root_cause().to_string();
            err!(ui, "Command error: {}", message);
            // runtime apis do not allow us to recover from this error
            // and does not provide machine-readable error codes
            if message.find("Broken pipe (os error 32)").is_some() {
                break;
            }
        } else {
            let _ = rx.next().await;
        }
    }

    info!(ui, "Shutting down...");
    if let Err(e) = service.shutdown().await {
        err!(ui, "Shutdown error: {:?}", e);
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

    service
        .run_process(run_process)
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;

    Ok(())
}

fn runtime_command(args: &Args) -> Result<Command> {
    let rt_dir = args
        .runtime
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid runtime parent directory"))?;
    let mut command = Command::new(&args.runtime);
    command.current_dir(rt_dir);
    Ok(command)
}

struct UI<T: Terminal> {
    interface: Arc<Interface<T>>,
    history_path: PathBuf,
}

impl<T: Terminal> Clone for UI<T> {
    fn clone(&self) -> Self {
        Self {
            interface: self.interface.clone(),
            history_path: self.history_path.clone(),
        }
    }
}

impl<T: Terminal + 'static> UI<T> {
    pub fn new<P: AsRef<Path>>(interface: Interface<T>, history_path: P) -> Result<Self> {
        {
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(&history_path)
                .context("unable to create a command history file")?;
        }

        interface.load_history(&history_path)?;
        interface.set_prompt(&format!(
            "\x01{prefix}\x02{text}\x01{suffix}\x02",
            prefix = (*COLOR_PROMPT).prefix(),
            text = "\n▶ ",
            suffix = (*COLOR_PROMPT).suffix()
        ))?;

        [
            Signal::Break,
            Signal::Interrupt,
            Signal::Continue,
            Signal::Suspend,
            Signal::Quit,
        ]
        .iter()
        .for_each(|s| interface.set_report_signal(*s, true));

        Ok(Self {
            interface: Arc::new(interface),
            history_path: history_path.as_ref().to_path_buf(),
        })
    }

    async fn enter_prompt(&mut self, mut tx: mpsc::Sender<String>) {
        while let Ok(ReadResult::Input(line)) = self.read_line() {
            if !line.trim().is_empty() {
                self.add_history(&line);
                let _ = tx.send(line).await;
            }
        }
    }

    pub fn close(&mut self) {
        if let Err(e) = self.interface.save_history(&self.history_path) {
            err!(self, "Error saving history to file: {}", e);
        }
        let _ = self.interface.set_prompt("");
        let _ = self.interface.cancel_read_line();
    }

    fn read_line(&self) -> std::io::Result<ReadResult> {
        self.interface.read_line()
    }

    fn add_history<S: AsRef<str>>(&mut self, entry: S) {
        self.interface.add_history(entry.as_ref().to_string());
    }

    fn write_fmt(&self, args: std::fmt::Arguments) -> std::io::Result<()> {
        let s = args.to_string();
        self.interface
            .lock_writer_erase()
            .expect("unable to get writer")
            .write_str(&s)
    }
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let mut args = Args::from_args();
    args.runtime = args.runtime.canonicalize().context("runtime not found")?;

    let work_dir = DataDir::new("ya-provider")
        .get_or_create()
        .context("unable to open data dir")?;
    let history_path = work_dir.join(".dbg_history");
    let mut ui = UI::new(Interface::new("ui")?, history_path)?;

    let rt_args = args
        .to_runtime_args()
        .into_iter()
        .map(|s: OsString| s.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    misc!(
        ui,
        "Invocation : {} {}",
        args.runtime.display(),
        rt_args.join(" ")
    );

    if args.deploy {
        deploy(&args, ui.clone())
            .await
            .context("deployment failed")?;
    }

    let (started_tx, started_rx) = oneshot::channel();
    let (input_tx, input_rx) = mpsc::channel(1);

    std::thread::spawn({
        let ui = ui.clone();
        move || {
            System::new("runtime").block_on(async move {
                if let Err(e) = start(args, input_rx, started_tx, ui.clone()).await {
                    err!(ui, "Runtime error: {}", e);
                }
            })
        }
    });

    started_rx.await?;
    ui.enter_prompt(input_tx).await;
    ui.close();

    Ok(())
}
