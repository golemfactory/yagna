use futures::future::BoxFuture;
use futures::prelude::*;
use futures::FutureExt;
use std::{
    clone::Clone,
    env,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio;
use ya_runtime_api::server::{self, AsyncResponse, ProcessStatus, RuntimeEvent, RuntimeService};

// server

struct RuntimeMock<E>
where
    E: RuntimeEvent,
{
    event_emitter: E,
}

impl<E: RuntimeEvent> server::RuntimeService for RuntimeMock<E> {
    fn hello(&self, version: &str) -> AsyncResponse<String> {
        eprintln!("server version: {}", version);
        async { Ok("0.0.0-demo".to_owned()) }.boxed_local()
    }

    fn run_process(
        &self,
        _run: server::RunProcess,
    ) -> server::AsyncResponse<server::RunProcessResp> {
        async move {
            let mut resp: server::RunProcessResp = Default::default();
            resp.pid = 100;
            log::debug!("before delay_for");
            tokio::time::delay_for(Duration::from_secs(3)).await;
            log::debug!("after delay_for");
            self.event_emitter
                .on_process_status(ProcessStatus {
                    pid: resp.pid,
                    running: true,
                    return_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
                .await;
            Ok(resp)
        }
        .boxed_local()
    }

    fn kill_process(&self, kill: server::KillProcess) -> AsyncResponse<()> {
        log::debug!("got kill: {:?}", kill);
        future::ok(()).boxed_local()
    }

    fn shutdown(&self) -> AsyncResponse<'_, ()> {
        log::debug!("got shutdown");
        future::ok(()).boxed_local()
    }
}

// client

// holds last received status
struct EventMock(Arc<Mutex<ProcessStatus>>);

impl EventMock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Default::default())))
    }

    fn get_last_status(&self) -> ProcessStatus {
        self.0.lock().unwrap().clone()
    }
}

impl RuntimeEvent for EventMock {
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()> {
        log::debug!("event: {:?}", status);
        *(self.0.lock().unwrap()) = status;
        future::ready(()).boxed()
    }
}

impl Clone for EventMock {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    env_logger::init();
    if env::var("X_SERVER").is_ok() {
        server::run(|event_emitter| RuntimeMock { event_emitter }).await
    } else {
        use tokio::process::Command;
        let exe = env::current_exe().unwrap();

        let mut cmd = Command::new(exe);
        cmd.env("X_SERVER", "1");
        let events = EventMock::new();
        let c = server::spawn(cmd, events.clone()).await?;
        log::debug!("hello_result={:?}", c.hello("0.0.0x").await);
        let mut run = server::RunProcess::default();
        run.bin = "sleep".to_owned();
        run.args = vec!["10".to_owned()];
        let sleep_1 = c.run_process(run.clone());
        let sleep_2 = c.run_process(run.clone());
        let sleep_3 = c.run_process(run);
        log::info!("start sleep1");
        log::info!("sleep1={:?}", sleep_1.await);
        log::info!("start sleep2 sleep3");
        log::info!("sleep23={:?}", future::join(sleep_2, sleep_3).await);
        log::info!("last status: {:?}", events.get_last_status());
    }
    Ok(())
}
