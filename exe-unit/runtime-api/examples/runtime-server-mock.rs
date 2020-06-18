use futures::prelude::*;
use std::env;
use std::process::Command;
use tokio;
use ya_runtime_api::server;
use ya_runtime_api::server::AsyncResponse;
use ya_runtime_api::server::RuntimeService;
use std::time::Duration;

struct RuntimeMock;

impl server::RuntimeService for RuntimeMock {
    fn hello(&self, version: &str) -> AsyncResponse<String> {
        eprintln!("server version: {}", version);
        async { Ok("0.0.0-demo".to_owned()) }.boxed_local()
    }

    fn run_process(
        &self,
        run: server::RunProcess,
    ) -> server::AsyncResponse<server::RunProcessResp> {
        async {
            let mut resp : server::RunProcessResp = Default::default();
            resp.pid = 100;
            log::debug!("before delay_for");
            tokio::time::delay_for(Duration::from_secs(3)).await;
            log::debug!("after delay_for");
            Ok(resp)
        }.boxed_local()
    }

    fn kill_process(&self, kill: server::KillProcess) -> AsyncResponse<()> {
        unimplemented!()
    }
}

#[tokio::main]
async fn main() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    env_logger::init();
    if env::var("X_SERVER").is_ok() {
        server::run(RuntimeMock).await
    } else {
        use tokio::process::Command;
        let exe = env::current_exe().unwrap();

        let mut cmd = Command::new(exe);
        cmd.env("X_SERVER", "1");
        let c = server::spawn(cmd).await;
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
        log::info!("sleep23={:?}",future::join(sleep_2, sleep_3).await);
    }
}
