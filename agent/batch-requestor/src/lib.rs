use actix::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub enum WasmRuntime {
    Wasi(i32), /* Wasi version */
}

pub struct ImageSpec {
    runtime: WasmRuntime,
    /* TODO */
}

impl ImageSpec {
    pub fn from_github<T: Into<String>>(_github_repository: T) -> Self {
        Self {
            runtime: WasmRuntime::Wasi(1),
        }
        /* TODO connect and download image specification */
    }
    pub fn runtime(self, runtime: WasmRuntime) -> Self {
        Self { runtime }
    }
}

pub enum Command {
    Deploy,
    Start,
    Run(Vec<String>),
    Stop,
}

pub mod command_helpers {
    use crate::Command;
    #[allow(non_upper_case_globals)]
    pub const deploy: Command = Command::Deploy;
    #[allow(non_upper_case_globals)]
    pub const start: Command = Command::Start;
    #[allow(non_upper_case_globals)]
    pub const stop: Command = Command::Stop;
    pub fn run(s: &[&str]) -> Command {
        Command::Run(s.into_iter().map(|s| s.to_string()).collect())
    }
}

pub struct CommandList(Vec<Command>);

impl CommandList {
    pub fn new(v: Vec<Command>) -> Self {
        Self(v)
    }
}

pub struct TaskSession {
    name: String,
    timeout: Duration,
    demand: Option<WasmDemand>,
    tasks: Vec<CommandList>,
}

impl TaskSession {
    pub fn new<T: Into<String>>(name: T) -> Self {
        Self {
            name: name.into(),
            timeout: Duration::from_secs(60),
            demand: None,
            tasks: vec![],
        }
    }
    pub fn with_timeout(self, timeout: std::time::Duration) -> Self {
        Self { timeout, ..self }
    }
    pub fn demand(self, demand: WasmDemand) -> Self {
        Self {
            demand: Some(demand),
            ..self
        }
    }
    pub fn tasks<T: std::iter::Iterator<Item = CommandList>>(self, tasks: T) -> Self {
        Self {
            tasks: tasks.collect(),
            ..self
        }
    }
    pub fn run(self) -> Addr<TaskSession> {
        /* TODO 1. download image spec (demand.spec)
        2. market api -> subscribe
        3. activity_api -> */
        self.start()
    }
}

pub struct WasmDemand {
    spec: ImageSpec,
    min_ram_gib: f64,
    min_storage_gib: f64,
}

impl WasmDemand {
    pub fn with_image(spec: ImageSpec) -> Self {
        Self {
            spec,
            min_ram_gib: 0.0,
            min_storage_gib: 0.0,
        }
    }
    pub fn min_ram_gib<T: Into<f64>>(self, min_ram_gib: T) -> Self {
        Self {
            min_ram_gib: min_ram_gib.into(),
            ..self
        }
    }
    pub fn min_storage_gib<T: Into<f64>>(self, min_storage_gib: T) -> Self {
        Self {
            min_storage_gib: min_storage_gib.into(),
            ..self
        }
    }
}

#[macro_export]
macro_rules! commands {
    ( $( $cmd:expr );* ; ) => {{
        let mut v = Vec::new();
        $(
            v.push($cmd);
        )*
        CommandList::new(v)
    }}
}

impl Actor for TaskSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        eprintln!(
            "BatchRequestor started. Timeout: {:?}. Demand: {:?}.",
            self.timeout,
            self.demand.is_some()
        );
    }
}

struct GetStatus {}

impl Message for GetStatus {
    type Result = f32;
}

impl Handler<GetStatus> for TaskSession {
    type Result = f32;

    fn handle(&mut self, msg: GetStatus, ctx: &mut Self::Context) -> Self::Result {
        unimplemented!()
    }
}

pub async fn tui_progress_monitor(task_session: Addr<TaskSession>) -> Result<(), ()> {
    /* TODO attach to the actor */
    let progress_bar = ProgressBar::new(100);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .progress_chars("=> ")
            .template("{elapsed_precise} [{bar:40}] {msg}"),
    );
    progress_bar.set_message("Running tasks");
    for _ in 0..100 {
        progress_bar.inc(1);
        std::thread::sleep(Duration::from_millis(55));
    }
    progress_bar.finish();
    Ok(())
}
