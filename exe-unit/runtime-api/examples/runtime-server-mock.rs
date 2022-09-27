use futures::future::BoxFuture;
use futures::prelude::*;
use futures::FutureExt;
use std::clone::Clone;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ya_runtime_api::server::*;

// server

struct RuntimeMock<H>
where
    H: RuntimeHandler,
{
    handler: H,
}

impl<H: RuntimeHandler> RuntimeService for RuntimeMock<H> {
    fn hello(&self, version: &str) -> AsyncResponse<String> {
        eprintln!("server version: {}", version);
        async { Ok("0.0.0-demo".to_owned()) }.boxed_local()
    }

    fn run_process(&self, _run: RunProcess) -> AsyncResponse<RunProcessResp> {
        async move {
            let resp = RunProcessResp { pid: 100 };
            log::debug!("before sleep");
            tokio::time::sleep(Duration::from_secs(3)).await;
            log::debug!("after sleep");
            self.handler
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

    fn kill_process(&self, kill: KillProcess) -> AsyncResponse<()> {
        log::debug!("got kill: {:?}", kill);
        future::ok(()).boxed_local()
    }

    fn create_network(&self, _: CreateNetwork) -> AsyncResponse<'_, CreateNetworkResp> {
        unimplemented!()
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

impl RuntimeHandler for EventMock {
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()> {
        log::debug!("event: {:?}", status);
        *(self.0.lock().unwrap()) = status;
        future::ready(()).boxed()
    }

    fn on_runtime_status<'a>(&self, _: RuntimeStatus) -> BoxFuture<'a, ()> {
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
        run(|event_emitter| RuntimeMock {
            handler: event_emitter,
        })
        .await
    } else {
        use tokio::process::Command;
        let exe = env::current_exe().unwrap();

        let mut cmd = Command::new(exe);
        cmd.env("X_SERVER", "1");
        let events = EventMock::new();
        let c = spawn(cmd, events.clone()).await?;
        log::debug!("hello_result={:?}", c.hello("0.0.0x").await);
        let run = RunProcess {
            bin: "sleep".to_owned(),
            args: vec!["10".to_owned()],
            ..Default::default()
        };
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
