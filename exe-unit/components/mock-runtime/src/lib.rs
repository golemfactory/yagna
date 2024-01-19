pub mod testing;

use futures::future::BoxFuture;
use futures::prelude::*;
use futures::FutureExt;
use std::clone::Clone;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ya_runtime_api::server::*;

pub struct RuntimeMock<H>
where
    H: RuntimeHandler,
{
    pub handler: H,
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
#[derive(Clone)]
pub struct EventMock(Arc<Mutex<ProcessStatus>>);

impl EventMock {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Default::default())))
    }

    pub fn get_last_status(&self) -> ProcessStatus {
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
