use crate::appkey;
use crate::setup::RunConfig;
use anyhow::{bail, Context, Result};
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use std::io;

use crate::utils::is_yagna_running;

use crate::command::YaCommand;
use std::process::ExitStatus;
use tokio::process::Child;
use tokio::stream::StreamExt;
use tokio::time::Duration;

fn handle_ctrl_c(result: io::Result<()>) -> Result<()> {
    if result.is_ok() {
        log::info!("Got ctrl+c. Bye!");
    }
    result.context("Couldn't listen to signals")?;
    Ok(())
}

struct AbortableChild(Option<oneshot::Sender<oneshot::Sender<io::Result<ExitStatus>>>>);

impl AbortableChild {
    fn new(child: Child, mut kill_cmd: mpsc::Sender<()>, name: &'static str) -> Self {
        let (tx, rx) = oneshot::channel();

        tokio::task::spawn_local(async move {
            match future::select(child, rx).await {
                future::Either::Left((result, _)) => {
                    log::error!("child {} exited too early: {:?}", name, result);
                    if kill_cmd.send(()).await.is_err() {
                        log::warn!("unable to send end-of-process notification");
                    }
                }
                future::Either::Right((
                    Ok::<oneshot::Sender<io::Result<ExitStatus>>, oneshot::Canceled>(tx),
                    mut child,
                )) => {
                    if let Err(e) = child.kill() {
                        log::error!("unable to kill {}: {:?}", name, e);
                        let _ = tx.send(Err(e));
                    } else {
                        let exit_status = child.await;
                        let _ = tx.send(exit_status);
                    }
                }
                future::Either::Right((Err(_), mut child)) => {
                    let _ = child.kill();
                    let _ = child.await;
                }
            }
        });

        Self(Some(tx))
    }

    async fn abort(&mut self) -> io::Result<ExitStatus> {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.take().unwrap().send(tx);
        rx.await
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "process exited too early"))?
    }
}

pub async fn watch_for_vm() -> anyhow::Result<()> {
    let cmd = YaCommand::new()?;
    let presets = cmd.ya_provider()?.list_presets().await?;
    if !presets.iter().any(|p| p.exeunit_name == "vm") {
        return Ok(());
    }
    let mut status = crate::platform::kvm_status();

    cmd.ya_provider()?
        .set_profile_activity("vm", status.is_valid())
        .await
        .ok();

    loop {
        tokio::time::delay_for(Duration::from_secs(60)).await;
        let new_status = crate::platform::kvm_status();
        if new_status.is_valid() != status.is_valid() {
            cmd.ya_provider()?
                .set_profile_activity("vm", new_status.is_valid())
                .await
                .ok();
            log::info!("Changed vm status to {:?}", new_status.is_valid());
        }
        status = new_status
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

    let (event_tx, mut event_rx) = mpsc::channel(1);
    let mut service = AbortableChild::new(service, event_tx.clone(), "yagna");
    let mut provider = AbortableChild::new(provider, event_tx, "provider");

    futures::pin_mut!(ctrl_c);
    //futures::pin_mut!(event_rx);
    tokio::task::spawn_local(async move {
        if let Err(e) = watch_for_vm().await {
            log::error!("vm checker failed: {:?}", e)
        }
    });

    if let future::Either::Left((r, _)) =
        future::select(ctrl_c, StreamExt::next(&mut event_rx)).await
    {
        let _ignore = handle_ctrl_c(r);
    }

    if let Err(e) = provider.abort().await {
        log::warn!("provider exited with: {:?}", e);
    }
    if let Err(e) = service.abort().await {
        log::warn!("service exited with: {:?}", e);
    }
    Ok(0)
}
