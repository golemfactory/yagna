use crate::appkey;
use crate::command::{YaCommand, ERC20_DRIVER, NETWORK_GROUP_MAP, ZKSYNC_DRIVER};
use crate::setup::RunConfig;
use crate::utils::payment_account;
use anyhow::{Context, Result};
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::StreamExt;
use std::io;
use std::process::ExitStatus;
use tokio::process::Child;
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
    fn new(
        mut child: Child,
        mut kill_cmd: mpsc::Sender<()>,
        name: &'static str,
        send_term: bool,
    ) -> Self {
        let (tx, rx) = oneshot::channel();

        #[allow(unused)]
        async fn wait_and_kill(mut child: Child, send_term: bool) -> io::Result<ExitStatus> {
            #[cfg(target_os = "linux")]
            if send_term {
                use ::nix::sys::signal::*;
                use ::nix::unistd::Pid;

                match child.id() {
                    Some(id) => {
                        let _ret = ::nix::sys::signal::kill(Pid::from_raw(id as i32), SIGTERM);
                    }
                    None => log::error!("missing child process pid"),
                }
            }
            // Yagna service should get ~10 seconds to clean up
            match tokio::time::timeout(Duration::from_secs(15), child.wait()).await {
                Ok(r) => r,
                Err(_) => {
                    child.start_kill()?;
                    child.wait().await
                }
            }
        }

        tokio::task::spawn_local(async move {
            tokio::select! {
                r = child.wait() => {
                    log::error!("child {} exited too early: {:?}", name, r);
                    if kill_cmd.send(()).await.is_err() {
                        log::warn!("unable to send end-of-process notification");
                    }
                },
                r = rx => match r {
                    Ok::<oneshot::Sender<io::Result<ExitStatus>>, oneshot::Canceled>(tx) => {
                        let _ = tx.send(wait_and_kill(child, send_term).await);
                    },
                    Err(_) => {
                        let _ = wait_and_kill(child, send_term).await;
                    }
                }
            };
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
        tokio::time::sleep(Duration::from_secs(60)).await;
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

    let cmd = YaCommand::new()?;
    let service = cmd.yagna()?.service_run(&config).await?;
    let app_key = appkey::get_app_key().await?;

    let provider_config = cmd.ya_provider()?.get_config().await?;
    let address =
        payment_account(&cmd, &config.account.account.or(provider_config.account)).await?;
    for nn in NETWORK_GROUP_MAP[&config.account.network].iter() {
        cmd.yagna()?
            .payment_init(&address, nn, &ERC20_DRIVER)
            .await?;
        if ZKSYNC_DRIVER.platform(nn).is_err() {
            continue;
        }
        if let Err(e) = cmd
            .yagna()?
            .payment_init(&address, nn, &ZKSYNC_DRIVER)
            .await
        {
            log::debug!("Failed to initialize zkSync driver. e:{}", e);
        };
    }

    let provider = cmd.ya_provider()?.spawn(&app_key, &config).await?;
    let ctrl_c = tokio::signal::ctrl_c();

    log::info!("Golem provider is running");

    let (event_tx, mut event_rx) = mpsc::channel(1);
    let mut service = AbortableChild::new(service, event_tx.clone(), "yagna", true);
    let mut provider = AbortableChild::new(provider, event_tx, "provider", false);

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
        return Ok(11);
    }
    if let Err(e) = service.abort().await {
        log::warn!("service exited with: {:?}", e);
        return Ok(12);
    }
    Ok(0)
}

#[cfg(target_family = "unix")]
pub async fn stop() -> Result<i32> {
    use ya_utils_path::data_dir::DataDir;
    use ya_utils_process::lock::ProcLock;

    let provider_dir = DataDir::new("ya-provider")
        .get_or_create()
        .expect("unable to get ya-provider data dir");
    let provider_pid = ProcLock::new("ya-provider", &provider_dir)?.read_pid()?;

    kill_pid(provider_pid as i32, 5)
        .await
        .context("failed to stop provider")?;

    let yagna_dir = DataDir::new("yagna")
        .get_or_create()
        .expect("unable to get yagna data dir");
    let yagna_pid = ProcLock::new("yagna", &yagna_dir)?.read_pid()?;

    kill_pid(yagna_pid as i32, 5)
        .await
        .context("failed to stop yagna")?;

    Ok(0)
}

#[cfg(target_family = "unix")]
async fn kill_pid(pid: i32, timeout: i64) -> Result<()> {
    use nix::sys::signal::*;
    use nix::sys::wait::*;
    use nix::unistd::Pid;
    use std::time::Instant;

    fn alive(pid: Pid) -> bool {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) | Err(_) => false,
            Ok(_) => true,
        }
    }

    let pid = Pid::from_raw(pid);
    let delay = Duration::from_secs_f32(timeout as f32 / 5.);
    let started = Instant::now();

    kill(pid, Signal::SIGTERM)?;
    log::debug!("Sent SIGTERM to {:?}", pid);

    while alive(pid) {
        if Instant::now() >= started + delay {
            log::debug!("Sending SIGKILL to {:?}", pid);

            kill(pid, Signal::SIGKILL)?;
            waitpid(pid, None)?;
            break;
        }
        tokio::time::sleep(delay).await;
    }
    Ok(())
}

#[cfg(not(target_family = "unix"))]
pub async fn stop() -> Result<i32> {
    // FIXME: not implemented for windows
    todo!("Implement for Windows");
}
