use crate::appkey;
use crate::setup::RunConfig;
use anyhow::{bail, Context, Result};
use futures::{future::FutureExt, select};
use std::io;

use crate::utils::is_yagna_running;

use crate::command::YaCommand;

fn handle_ctrl_c(result: io::Result<()>) -> Result</*exit code*/ i32> {
    if result.is_ok() {
        log::info!("Got ctrl+c. Bye!");
    }
    result.context("Couldn't listen to signals")?;
    Ok(0)
}

fn handle_subprocess(
    name: &str,
    result: io::Result<std::process::ExitStatus>,
) -> Result</*exit code*/ i32> {
    match result {
        Ok(exit_status) => {
            bail!("{} exited too early, {}", name, exit_status);
        }
        Err(e) => {
            bail!("Failed to spawn {}: {}", name, e);
        }
    }
}

pub async fn run(mut config: RunConfig) -> Result</*exit code*/ i32> {
    crate::setup::setup(&mut config, false).await?;
    if is_yagna_running().await? {
        bail!("service already running")
    }
    let cmd = YaCommand::new()?;

    let service = cmd.yagna()?.service_run().await?;

    let app_key = appkey::get_app_key().await?;
    let provider = cmd.ya_provider()?.spawn(&app_key).await?;

    let ctrl_c = tokio::signal::ctrl_c();

    log::info!("Golem provider is running");
    select!(
        result = ctrl_c.fuse() => handle_ctrl_c(result),
        result = service.fuse() => handle_subprocess("yagna service", result),
        result = provider.fuse() => handle_subprocess("ya-provider", result),
    )
}
