use futures::future;
use std::env;

use ya_mock_runtime::{EventMock, RuntimeMock};
use ya_runtime_api::server::{run, spawn, RunProcess, RuntimeService};

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
