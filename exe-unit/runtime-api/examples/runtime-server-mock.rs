use futures::prelude::*;
use std::env;

use std::time::Duration;
use tokio;
use ya_runtime_api::server;
use ya_runtime_api::server::AsyncResponse;
use ya_runtime_api::server::RuntimeService;

struct RuntimeMock;

struct EventMock;

impl server::RuntimeService for RuntimeMock {
    fn hello(&self, version: &str) -> AsyncResponse<String> {
        eprintln!("server version: {}", version);
        async { Ok("0.0.0-demo".to_owned()) }.boxed_local()
    }

    fn run_process(
        &self,
        _run: server::RunProcess,
    ) -> server::AsyncResponse<server::RunProcessResp> {
        async {
            let mut resp: server::RunProcessResp = Default::default();
            resp.pid = 100;
            log::debug!("before delay_for");
            tokio::time::delay_for(Duration::from_secs(3)).await;
            log::debug!("after delay_for");
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

impl server::RuntimeEvent for EventMock {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    env_logger::init();
    if env::var("X_SERVER").is_ok() {
        server::run(|_e| RuntimeMock).await
    } else {
        use tokio::process::Command;
        let exe = env::current_exe().unwrap();

        let mut cmd = Command::new(exe);
        cmd.env("X_SERVER", "1");
        let c = server::spawn(cmd, EventMock).await?;
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
    }
    Ok(())
}
